#!/usr/bin/env python3
"""Emit an advisory GitHub Actions warning when a PR omits CHANGELOG.md."""

from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import sys


WARNING = (
    "::warning title=CHANGELOG.md not changed::"
    "This pull request does not modify CHANGELOG.md. "
    "Update [Unreleased] with human-reviewed release-note text, or mark the "
    "PR template changelog-skip checkbox with a justification."
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--event-name", required=True)
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", default="HEAD")
    parser.add_argument("--repo", type=Path, default=Path("."))
    return parser.parse_args()


def changed_files(repo: Path, base: str, head: str) -> list[str]:
    result = subprocess.run(
        ["git", "-C", str(repo), "diff", "--name-only", f"{base}...{head}"],
        check=True,
        text=True,
        capture_output=True,
    )
    return [line for line in result.stdout.splitlines() if line]


def main() -> int:
    args = parse_args()
    if args.event_name != "pull_request":
        print(f"changelog advisory skipped for event {args.event_name}")
        return 0
    files = changed_files(args.repo, args.base, args.head)
    if not files:
        print("changelog advisory: no changed files")
        return 0
    if "CHANGELOG.md" in files:
        print("changelog advisory: CHANGELOG.md changed")
        return 0
    print(WARNING)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
