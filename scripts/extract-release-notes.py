#!/usr/bin/env python3
"""Extract the reviewed CHANGELOG.md section for a release tag."""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import sys


def extract_release_notes(changelog: str, tag: str) -> str:
    header = re.compile(rf"^## \[{re.escape(tag)}\] — \d{{4}}-\d{{2}}-\d{{2}}\s*$")
    next_header = re.compile(r"^## \[")
    found = False
    body: list[str] = []
    for line in changelog.splitlines():
        if not found:
            if header.match(line):
                found = True
            continue
        if next_header.match(line):
            break
        body.append(line)
    notes = "\n".join(body).strip()
    if notes:
        return notes + "\n"
    return ""


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Extract a human-reviewed release notes section from CHANGELOG.md",
    )
    parser.add_argument("--changelog", type=Path, default=Path("CHANGELOG.md"))
    parser.add_argument("--tag", required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument(
        "--print",
        action="store_true",
        help="Print extracted notes to stdout for workflow operator visibility",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    notes = extract_release_notes(args.changelog.read_text(encoding="utf-8"), args.tag)
    if not notes:
        print(
            f"::error::CHANGELOG.md has no reviewed release section for {args.tag}",
            file=sys.stderr,
        )
        print(
            f"Expected header: ## [{args.tag}] — YYYY-MM-DD",
            file=sys.stderr,
        )
        return 1
    args.out.write_text(notes, encoding="utf-8")
    if args.print:
        print(notes, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
