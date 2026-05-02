#!/usr/bin/env python3
"""Backfill acceptance criteria on the 246 leaves created by 02-author-leaves.py.

The first parser created the beads but failed every `br update --acceptance-criteria`
because clap rejects values starting with `- [` as suspected flags. This script
re-runs the update with `--acceptance-criteria=<value>` (= syntax bypasses the
ambiguity).

Maps each leaf back to its bead id by (parent_id, title). Idempotent.
"""
from __future__ import annotations

import importlib.util
import json
import os
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

SPECS = Path("/tank/projects/m80/specs")
spec = importlib.util.spec_from_file_location("author", str(SPECS / "02-author-leaves.py"))
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

ids = mod.load_ids(SPECS / "skeleton-ids.env")

# Build (parent_id, title) -> bead_id from the live workspace.
result = subprocess.run(
    ["br", "list", "--type", "task", "--limit", "1000", "--json"],
    capture_output=True, text=True, check=True,
)
data = json.loads(result.stdout)
issues = data.get("issues", [])

by_parent: dict[str, dict[str, str]] = defaultdict(dict)
for it in issues:
    bid = it["id"]
    parent_id = bid.rsplit(".", 1)[0]  # bead 'm80-a7g.1.5' → parent 'm80-a7g.1'
    title = it["title"]
    by_parent[parent_id][title] = bid

print(f"Loaded {len(issues)} task beads under {len(by_parent)} distinct parents", file=sys.stderr)

logf = (SPECS / "02b-backfill.log").open("w")
total = 0
ok = 0
fail = 0
not_found = 0
already_set = 0

for spec_file in sorted(SPECS.glob("leaves-*.md")):
    text = spec_file.read_text()
    leaves = list(mod.parse_leaves(text))
    print(f"== {spec_file.name}: {len(leaves)} leaves", file=sys.stderr)

    for entry in leaves:
        total += 1
        parent_id = mod.expand(entry["parent_var"], ids)
        title = entry["title"].strip()
        bead_id = by_parent.get(parent_id, {}).get(title)
        if not bead_id:
            not_found += 1
            line = f"NOT_FOUND parent={parent_id} title={title[:60]!r}"
            logf.write(line + "\n")
            print(line, file=sys.stderr)
            continue

        acceptance = mod.make_acceptance(entry.get("captured_by", ""))
        # Use --key=value to avoid clap rejecting '- [' as a flag.
        cmd = ["br", "update", bead_id, f"--acceptance-criteria={acceptance}"]
        try:
            r = subprocess.run(cmd, capture_output=True, text=True, check=False, timeout=30)
        except subprocess.TimeoutExpired:
            fail += 1
            line = f"TIMEOUT {bead_id} title={title[:60]!r}"
            logf.write(line + "\n")
            print(line, file=sys.stderr)
            continue
        if r.returncode != 0:
            fail += 1
            line = f"ERR {bead_id} exit={r.returncode}: {r.stderr.strip()[:150]}"
            logf.write(line + "\n")
            print(line, file=sys.stderr)
        else:
            ok += 1
            logf.write(f"OK {bead_id} <- {title[:60]}\n")

print(f"\nTotal: {total}, ok: {ok}, fail: {fail}, not_found: {not_found}", file=sys.stderr)
logf.write(f"\nTotal: {total}, ok: {ok}, fail: {fail}, not_found: {not_found}\n")
logf.close()
sys.exit(0 if (fail == 0 and not_found == 0) else 2)
