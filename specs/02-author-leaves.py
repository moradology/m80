#!/usr/bin/env python3
"""Parse leaves-*.md files in /tank/projects/m80/specs/ and create one bead per leaf.

Expected leaf format in each markdown file:

    ### Leaf: <title>
    - parent_var: $L2_01_1
    - labels: $ACTIVE,preflight,binaries
    - status: open                            # open | deferred
    - behavior: <one-sentence present-tense fact>
    - source: <citation>
    - captured-by: <doc path> + <test path>

Multiline values (e.g., for `behavior`, `source`, `captured-by`) MAY span
multiple lines as long as the continuation lines are indented (start with
spaces) and don't begin with `- ` or `### `.

Run from /tank/projects/m80/. Idempotent? NO — re-running creates duplicates.

Behavior:
- Loads /tank/projects/m80/specs/skeleton-ids.env for L2 variable → bead id mapping.
- Walks /tank/projects/m80/specs/leaves-*.md.
- For each leaf, runs `br create` with title/type/priority/parent/labels/description/acceptance_criteria/status.
- Description = "Behavior: ...\nSource: ...\nCaptured by: ..." (plain triple).
- Acceptance criteria = standard 3-checkbox template with the leaf's captured-by paths interpolated.
- Tracks success/failure; writes a per-leaf log to specs/02-author-leaves.log.
"""
from __future__ import annotations

import os
import re
import subprocess
import sys
from pathlib import Path
from typing import Iterator

SPECS_DIR = Path("/tank/projects/m80/specs")
IDS_FILE = SPECS_DIR / "skeleton-ids.env"
LOG_FILE = SPECS_DIR / "02-author-leaves.log"

LABEL_VARS = {
    "ACTIVE": "firecracker,behavior-capture,active",
    "DEFERRED_V02": "firecracker,behavior-capture,deferred-v02",
    "DEFERRED_DEAD": "firecracker,behavior-capture,deferred-deadcode",
}


def load_ids(path: Path) -> dict[str, str]:
    ids: dict[str, str] = {}
    for raw in path.read_text().splitlines():
        line = raw.strip()
        if not line or "=" not in line:
            continue
        k, v = line.split("=", 1)
        ids[k.strip()] = v.strip()
    return ids


def expand(value: str, ids: dict[str, str]) -> str:
    """Expand $LABEL_VAR and $L2_xx_y references in a value."""
    out = value
    for key, replacement in LABEL_VARS.items():
        out = out.replace(f"${key}", replacement)
    # L2 / L1 ids
    def repl(match: re.Match[str]) -> str:
        var = match.group(0)[1:]  # drop leading $
        if var in ids:
            return ids[var]
        raise KeyError(f"unknown variable {match.group(0)} in '{value}'")

    out = re.sub(r"\$L[12]_\w+", repl, out)
    return out


def parse_leaves(text: str) -> Iterator[dict[str, str]]:
    """Parse a single leaves-*.md file and yield dicts per leaf."""
    lines = text.splitlines()
    i = 0
    n = len(lines)
    while i < n:
        line = lines[i]
        if line.startswith("### Leaf:"):
            title = line[len("### Leaf:") :].strip()
            entry: dict[str, str] = {"title": title}
            i += 1
            current_field: str | None = None
            current_buf: list[str] = []

            def flush() -> None:
                nonlocal current_field, current_buf
                if current_field:
                    entry[current_field] = " ".join(current_buf).strip()
                current_field = None
                current_buf = []

            while i < n:
                cur = lines[i]
                if cur.startswith("### ") or cur.startswith("## ") or cur.startswith("# "):
                    flush()
                    break
                m = re.match(r"-\s+([a-z_-]+):\s*(.*)$", cur)
                if m:
                    flush()
                    current_field = m.group(1).replace("-", "_")
                    current_buf = [m.group(2).strip()]
                elif current_field and cur.startswith(("  ", "\t")):
                    current_buf.append(cur.strip())
                elif cur.strip() == "":
                    pass
                i += 1
            flush()
            yield entry
        else:
            i += 1


def make_acceptance(captured_by: str) -> str:
    """captured-by is 'docpath + testpath'. Split and interpolate."""
    parts = [p.strip() for p in captured_by.split("+", 1)]
    if len(parts) == 2:
        doc_path, test_path = parts
    else:
        doc_path = captured_by.strip()
        test_path = "(test path: TBD)"
    return (
        f"- [ ] Behavior is documented in {doc_path}, with present-tense "
        "statement and predecessor source citation.\n"
        f"- [ ] Behavior is verified by an m80 test or fixture "
        f"({test_path}) that fails if behavior regresses.\n"
        "- [ ] Both artifacts referenced in this bead's notes field at "
        "close time."
    )


def make_description(entry: dict[str, str]) -> str:
    behavior = entry.get("behavior", "")
    source = entry.get("source", "")
    captured = entry.get("captured_by", "")
    return f"Behavior: {behavior}\nSource: {source}\nCaptured by: {captured}"


def br_create(entry: dict[str, str], ids: dict[str, str]) -> tuple[bool, str]:
    """Run `br create` with the given leaf entry. Returns (ok, msg)."""
    parent_var = entry.get("parent_var", "")
    if not parent_var:
        return False, "missing parent_var"
    parent_id = expand(parent_var, ids)

    labels = expand(entry.get("labels", ""), ids)
    status = entry.get("status", "open").strip()
    title = entry["title"].strip()
    description = make_description(entry)
    acceptance = make_acceptance(entry.get("captured_by", ""))

    create_cmd = [
        "br", "create", title,
        "--type", "task",
        "--priority", "2",
        "--parent", parent_id,
        "--labels", labels,
        "--description", description,
        "--silent",
    ]
    if status and status != "open":
        create_cmd.extend(["--status", status])

    try:
        result = subprocess.run(create_cmd, capture_output=True, text=True, check=False, timeout=30)
    except subprocess.TimeoutExpired:
        return False, "br create timed out"
    if result.returncode != 0:
        return False, f"create exit={result.returncode}: stderr={result.stderr.strip()}"
    new_id = result.stdout.strip()
    if not new_id:
        return False, "create returned empty id"

    # Set acceptance criteria via update.
    update_cmd = ["br", "update", new_id, "--acceptance-criteria", acceptance]
    try:
        upd = subprocess.run(update_cmd, capture_output=True, text=True, check=False, timeout=30)
    except subprocess.TimeoutExpired:
        return False, f"{new_id} (created; acceptance update timed out)"
    if upd.returncode != 0:
        return False, f"{new_id} (created; acceptance exit={upd.returncode}: {upd.stderr.strip()})"

    return True, new_id


def main() -> int:
    ids = load_ids(IDS_FILE)
    logf = LOG_FILE.open("w")
    total = 0
    failures = 0
    spec_files = sorted(SPECS_DIR.glob("leaves-*.md"))
    if not spec_files:
        print("ERROR: no leaves-*.md files in specs/", file=sys.stderr)
        return 1
    for spec in spec_files:
        text = spec.read_text()
        leaves = list(parse_leaves(text))
        print(f"== {spec.name}: {len(leaves)} leaves", file=sys.stderr)
        logf.write(f"\n== {spec.name}: {len(leaves)} leaves ==\n")
        for entry in leaves:
            total += 1
            ok, msg = br_create(entry, ids)
            status = "OK " if ok else "ERR"
            line = f"{status} [{entry.get('parent_var', '?')}] {entry['title'][:60]} -> {msg}"
            logf.write(line + "\n")
            if not ok:
                failures += 1
                print(line, file=sys.stderr)
        logf.flush()

    print(f"\nTotal: {total} leaves, {failures} failures", file=sys.stderr)
    logf.write(f"\nTotal: {total} leaves, {failures} failures\n")
    logf.close()
    return 0 if failures == 0 else 2


if __name__ == "__main__":
    sys.exit(main())
