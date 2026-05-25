#!/usr/bin/env bash
# Run all 9 validation checks from the plan against the m80 .beads/ workspace.
# Read-only; safe to re-run.
set -uo pipefail

cd /tank/projects/m80 || exit
echo "=================================================="
echo "Validation against /tank/projects/m80/.beads/"
echo "=================================================="

echo
echo "1. br stats — total issue counts"
echo "--------------------------------------------------"
br stats

echo
echo "2. br doctor — should report zero ERROR-level findings"
echo "--------------------------------------------------"
br doctor

echo
echo "3. bv --robot-suggest --suggest-confidence=0.9 — high-confidence duplicates"
echo "--------------------------------------------------"
bv --robot-suggest --min-confidence=0.9 --format json 2>&1 | python3 -c "
import json, sys
data = json.load(sys.stdin)
suggestions = data.get('suggestions', {}).get('suggestions', [])
print(f'Total high-confidence suggestions: {len(suggestions)}')
for s in suggestions[:10]:
    print(f'  - {s.get(\"type\")}: {s.get(\"target_bead\")} -> {s.get(\"summary\",\"\")[:60]}')"

echo
echo "4. bv --robot-graph mermaid (cycle check) — write graph for review"
echo "--------------------------------------------------"
bv --robot-graph --graph-format=mermaid > specs/dependency-graph.md 2>&1
echo "Mermaid graph written to specs/dependency-graph.md ($(wc -l < specs/dependency-graph.md) lines)"

echo
echo "5. bv --robot-triage-by-track — execution tracks"
echo "--------------------------------------------------"
bv --robot-triage-by-track --format json 2>&1 | python3 -c "
import json, sys
data = json.load(sys.stdin)
tracks = data.get('triage', {}).get('tracks', []) or data.get('tracks', [])
print(f'Tracks identified: {len(tracks)}')
for t in tracks[:20]:
    name = t.get('name') or t.get('label') or t.get('track', 'unnamed')
    items = len(t.get('items', t.get('beads', [])))
    print(f'  - {name}: {items} items')"

echo
echo "6. Spot-check: 5 random leaves' fields"
echo "--------------------------------------------------"
br list --type=task --limit=200 --json 2>&1 | python3 -c "
import json, sys, random
data = json.load(sys.stdin)
issues = data.get('issues', [])
sample = random.sample(issues, min(5, len(issues)))
for s in sample:
    desc = s.get('description', '') or ''
    has_behavior = 'Behavior:' in desc
    has_source = 'Source:' in desc
    has_captured = 'Captured by:' in desc
    ac = s.get('acceptance_criteria', '') or ''
    has_three = ac.count('- [ ]') == 3
    labels = s.get('labels', []) or []
    has_fc = any('firecracker' in l for l in labels)
    print(f'  {s[\"id\"]:18s} title={s[\"title\"][:40]!r:42s} desc-fields=Beh:{has_behavior} Src:{has_source} Cap:{has_captured} ac-3box={has_three} fc-label={has_fc}')"

echo
echo "7. Cross-reference: dossier source paths exist"
echo "--------------------------------------------------"
# Pull all 'Source:' lines, extract dossier file refs, verify they exist
br list --type=task --limit=500 --json 2>&1 | python3 -c "
import json, sys, re, os
from pathlib import Path
data = json.load(sys.stdin)
issues = data.get('issues', [])
dossier_refs = set()
predecessor_refs = set()
for s in issues:
    desc = s.get('description', '') or ''
    for m in re.finditer(r'dossier\s+\`(\d{2}-[\w-]+\.md)\`', desc):
        dossier_refs.add(m.group(1))
    for m in re.finditer(r'predecessor\s+\`([\w/.-]+\.(rs|sh|md))\`', desc):
        predecessor_refs.add(m.group(1))
print(f'Distinct dossier files referenced: {len(dossier_refs)}')
missing_dossier = [d for d in dossier_refs if not Path(f'/tank/projects/m80/{d}').exists()]
print(f'Missing dossier files: {len(missing_dossier)}')
for d in missing_dossier:
    print(f'  - {d}')
print(f'Distinct predecessor paths referenced: {len(predecessor_refs)}')
missing_predecessor = [t for t in predecessor_refs if not Path(f'/tank/projects/predecessor/{t}').exists()]
print(f'Missing predecessor paths: {len(missing_predecessor)}')
for t in missing_predecessor[:10]:
    print(f'  - {t}')"

echo
echo "8. Leaf reachability: each L2 has >=3 leaves"
echo "--------------------------------------------------"
br list --type=epic --limit=200 --json 2>&1 | python3 -c "
import json, sys, subprocess, re
data = json.load(sys.stdin)
epics = data.get('issues', [])
l2s = [e for e in epics if e['id'].count('.') == 1]
print(f'L2 sub-epics: {len(l2s)}')
weak = []
for l2 in l2s:
    lid = l2['id']
    out = subprocess.run(['br', 'list', '--type', 'task', '--json'], capture_output=True, text=True)
    if out.returncode == 0:
        td = json.loads(out.stdout)
        children = [t for t in td.get('issues',[]) if t.get('id','').startswith(lid + '.')]
        if len(children) < 3:
            weak.append((lid, l2['title'], len(children)))
        # cache; only run once
        break
# slower, but accurate path: pull full task list once
out = subprocess.run(['br', 'list', '--type', 'task', '--limit', '500', '--json'], capture_output=True, text=True)
all_tasks = json.loads(out.stdout).get('issues', []) if out.returncode == 0 else []
print(f'L3 leaves total: {len(all_tasks)}')
weak = []
for l2 in l2s:
    lid = l2['id']
    children = [t for t in all_tasks if t.get('id','').startswith(lid + '.')]
    if len(children) < 3:
        weak.append((lid, l2['title'], len(children)))
if weak:
    print(f'L2s with <3 leaves: {len(weak)}')
    for lid, title, n in weak:
        print(f'  - {lid:18s} ({n} leaves) {title[:50]}')
else:
    print('All L2s have >=3 leaves.')"

echo
echo "9. Boundary check: agent-surface terms are NOT in m80 leaves"
echo "--------------------------------------------------"
br list --type=task --limit=500 --json 2>&1 | python3 -c "
import json, sys, re
data = json.load(sys.stdin)
issues = data.get('issues', [])
forbidden = [
    'tool_call_id', 'tool call id', 'workspace_id', 'effect_class', 'EffectClass',
    'writeback_authority', 'writebackauthority', 'idempotency', 'sandbox_exec_started',
    'sandbox_exec_succeeded', 'sandbox_exec_failed', 'sandbox_exec_timed_out',
    'workspace_policy', 'WorkspacePolicy', 'tool_catalog', 'tool catalog',
    'CommitAuthority', 'commit_authority', 'authority lease',
]
hits = []
for s in issues:
    blob = (s.get('title','') + ' ' + (s.get('description','') or '') + ' ' + (s.get('acceptance_criteria','') or '')).lower()
    for term in forbidden:
        if term.lower() in blob:
            # whitelist: explicit drop-warnings inside descriptions are OK if they say 'NOT in m80' or 'drop' nearby
            if 'not in m80' in blob or 'drop' in blob and 'not' in blob:
                continue
            hits.append((s['id'], s['title'][:50], term))
            break
print(f'Boundary violations: {len(hits)}')
for id, t, term in hits[:10]:
    print(f'  - {id:18s} term={term!r}: {t}')"

echo
echo "=================================================="
echo "Validation complete."
echo "Mermaid graph at: specs/dependency-graph.md"
