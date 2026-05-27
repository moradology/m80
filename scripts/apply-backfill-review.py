#!/usr/bin/env python3
"""Apply human-reviewed v0.2 backfill sections to CHANGELOG.md."""

from __future__ import annotations

import argparse
import difflib
import importlib.util
from pathlib import Path
import re
import sys
from types import ModuleType


REPO_ROOT = Path(__file__).resolve().parents[1]
VERIFY_SCRIPT = REPO_ROOT / "scripts" / "verify-backfill-review.py"
RELEASE_HEADER_RE = re.compile(r"^## \[(v0\.2\.\d+)\] — \d{4}-\d{2}-\d{2}\s*$", re.MULTILINE)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Land reviewed v0.2 backfill sections into CHANGELOG.md",
    )
    parser.add_argument("--review", type=Path, default=Path("docs/release/backfill-review.md"))
    parser.add_argument("--changelog", type=Path, default=Path("CHANGELOG.md"))
    parser.add_argument(
        "--post-backfill-unreleased",
        type=Path,
        help="Body-only reviewed [Unreleased] content to keep after historical backfill",
    )
    parser.add_argument(
        "--empty-unreleased",
        action="store_true",
        help="Leave [Unreleased] empty because no post-backfill work remains",
    )
    parser.add_argument(
        "--expected-tags",
        help="Comma-separated v0.2.x tags for tests; defaults to git tags in the packet range",
    )
    parser.add_argument(
        "--skip-git-ranges",
        action="store_true",
        help="Skip local git tag date/commit-count verification",
    )
    parser.add_argument("--dry-run", action="store_true")
    return parser.parse_args()


def load_verifier() -> ModuleType:
    spec = importlib.util.spec_from_file_location("verify_backfill_review", VERIFY_SCRIPT)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load verifier: {VERIFY_SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def reviewed_unreleased_body(args: argparse.Namespace) -> str:
    if bool(args.post_backfill_unreleased) == bool(args.empty_unreleased):
        raise ValueError("pass exactly one of --post-backfill-unreleased or --empty-unreleased")
    if args.empty_unreleased:
        return ""
    body = args.post_backfill_unreleased.read_text(encoding="utf-8").strip()
    if body.startswith("## [Unreleased]"):
        raise ValueError("--post-backfill-unreleased must contain the section body only")
    if not body:
        raise ValueError("--post-backfill-unreleased is empty; use --empty-unreleased explicitly")
    return body


def render_sections(verifier: ModuleType, review_text: str, expected_tags: list[str]) -> list[str]:
    sections, section_errors = verifier.parse_sections(review_text)
    coverage, coverage_errors = verifier.parse_coverage(review_text)
    if section_errors or coverage_errors:
        raise ValueError("\n".join(section_errors + coverage_errors))
    rendered: list[str] = []
    for tag in reversed(expected_tags):
        row = coverage[tag]
        section = sections[tag]
        body = section.body.strip()
        rendered.append(f"## [{tag}] — {row.date}\n\n{body}\n")
    return rendered


def apply_backfill(changelog_text: str, unreleased_body: str, rendered_sections: list[str]) -> str:
    lines = changelog_text.splitlines()
    try:
        unreleased_idx = lines.index("## [Unreleased]")
    except ValueError as exc:
        raise ValueError("CHANGELOG.md has no ## [Unreleased] section") from exc

    next_release_idx = len(lines)
    for idx in range(unreleased_idx + 1, len(lines)):
        if lines[idx].startswith("## ["):
            next_release_idx = idx
            break

    existing_v02 = sorted(set(RELEASE_HEADER_RE.findall(changelog_text)))
    if existing_v02:
        raise ValueError(
            "CHANGELOG.md already contains v0.2 backfill sections: "
            + ", ".join(existing_v02)
        )

    unreleased_lines = ["## [Unreleased]"]
    if unreleased_body:
        unreleased_lines.extend(["", *unreleased_body.splitlines()])
    unreleased_lines.append("")

    section_lines: list[str] = []
    for section in rendered_sections:
        section_lines.extend(section.strip().splitlines())
        section_lines.append("")

    new_lines = (
        lines[:unreleased_idx]
        + unreleased_lines
        + section_lines
        + lines[next_release_idx:]
    )
    return "\n".join(new_lines).rstrip() + "\n"


def extract_section(changelog_text: str, tag: str) -> str:
    header = re.compile(rf"^## \[{re.escape(tag)}\] — \d{{4}}-\d{{2}}-\d{{2}}\s*$")
    found = False
    body: list[str] = []
    for line in changelog_text.splitlines():
        if not found:
            if header.match(line):
                found = True
            continue
        if line.startswith("## ["):
            break
        body.append(line)
    return "\n".join(body).strip()


def main() -> int:
    args = parse_args()
    try:
        unreleased_body = reviewed_unreleased_body(args)
        verifier = load_verifier()
        review_text = args.review.read_text(encoding="utf-8")
        errors = verifier.validate(
            review_text,
            expected_tags_arg=args.expected_tags,
            require_reviewed=True,
            check_git_ranges=not args.skip_git_ranges,
        )
        if errors:
            for error in errors:
                print(f"::error::{error}", file=sys.stderr)
            return 1
        expected_tags = verifier.expected_tags_for_packet(review_text, args.expected_tags)
        rendered_sections = render_sections(verifier, review_text, expected_tags)
        changelog_text = args.changelog.read_text(encoding="utf-8")
        next_changelog = apply_backfill(changelog_text, unreleased_body, rendered_sections)
        for tag in expected_tags:
            if not extract_section(next_changelog, tag):
                raise ValueError(f"generated CHANGELOG.md has empty release notes for {tag}")
    except ValueError as exc:
        print(f"::error::{exc}", file=sys.stderr)
        return 2

    if args.dry_run:
        diff = difflib.unified_diff(
            changelog_text.splitlines(keepends=True),
            next_changelog.splitlines(keepends=True),
            fromfile=str(args.changelog),
            tofile=f"{args.changelog} (backfilled)",
        )
        print("".join(diff), end="")
        return 0

    args.changelog.write_text(next_changelog, encoding="utf-8")
    print(f"applied reviewed backfill to {args.changelog}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
