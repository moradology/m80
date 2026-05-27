#!/usr/bin/env python3
"""Verify the v0.2 changelog backfill review packet."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path
import re
import subprocess
import sys


TAG_RE = re.compile(r"^v0\.2\.(\d+)$")
REQUIRED_RANGE_RE = re.compile(
    r"^Required stable tag range: (?P<first>v0\.2\.\d+) through (?P<last>v0\.2\.\d+) ",
    re.MULTILINE,
)
COVERAGE_ROW_RE = re.compile(
    r"^\| (?P<tag>v0\.2\.\d+) \| (?P<date>\d{4}-\d{2}-\d{2}) \| "
    r"(?P<range>[^|]+?) \| (?P<commits>\d+) \| (?P<status>[^|]+?) \|$",
    re.MULTILINE,
)
WRAPPER_RE = re.compile(r"^### (?P<tag>v0\.2\.\d+)\s*$", re.MULTILINE)
SECTION_HEADER_RE = re.compile(
    r"^## \[(?P<tag>v0\.2\.\d+)\] — (?P<date>\d{4}-\d{2}-\d{2})\s*$",
    re.MULTILINE,
)
CATEGORY_RE = re.compile(
    r"^### (Added|Changed|Deprecated|Removed|Fixed|Security|Performance|Documented)\s*$",
    re.MULTILINE,
)
REVIEWED_RE = re.compile(r"^human-reviewed by .+ on \d{4}-\d{2}-\d{2}$")
SECTION_MARKER_RE = re.compile(
    r"^(Draft suggestion — human review required|Human-reviewed by .+ on \d{4}-\d{2}-\d{2})$",
    re.MULTILINE,
)


@dataclass(frozen=True)
class CoverageRow:
    tag: str
    date: str
    range_text: str
    commits: int
    status: str


@dataclass(frozen=True)
class DraftSection:
    wrapper_tag: str
    range_text: str | None
    marker_status: str | None
    header_tag: str | None
    date: str | None
    body: str


def version_number(tag: str) -> int:
    match = TAG_RE.match(tag)
    if match is None:
        raise ValueError(f"unsupported tag format: {tag}")
    return int(match.group(1))


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Verify docs/release/backfill-review.md structure and review status",
    )
    parser.add_argument("--review", type=Path, default=Path("docs/release/backfill-review.md"))
    parser.add_argument(
        "--expected-tags",
        help="Comma-separated v0.2.x tags for tests; defaults to git tags in the packet range",
    )
    parser.add_argument(
        "--require-reviewed",
        action="store_true",
        help="Fail unless every coverage row has an explicit human-reviewed status",
    )
    parser.add_argument(
        "--check-git-ranges",
        action="store_true",
        help="Verify coverage dates and commit counts against local git tags",
    )
    return parser.parse_args()


def expected_tags_for_packet(text: str, expected_tags_arg: str | None) -> list[str]:
    if expected_tags_arg:
        tags = [tag.strip() for tag in expected_tags_arg.split(",") if tag.strip()]
    else:
        tags = git_v02_tags()
        range_match = REQUIRED_RANGE_RE.search(text)
        if range_match is not None:
            first = version_number(range_match.group("first"))
            last = version_number(range_match.group("last"))
            tags = [tag for tag in tags if first <= version_number(tag) <= last]
    if not tags:
        raise ValueError("no expected v0.2 tags found")
    invalid = [tag for tag in tags if TAG_RE.match(tag) is None]
    if invalid:
        raise ValueError(f"unsupported expected tag(s): {', '.join(invalid)}")
    return sorted(tags, key=version_number)


def git_v02_tags() -> list[str]:
    result = subprocess.run(
        ["git", "tag", "--list", "v0.2.*", "--sort=version:refname"],
        check=True,
        text=True,
        capture_output=True,
    )
    return [line for line in result.stdout.splitlines() if TAG_RE.match(line)]


def parse_coverage(text: str) -> tuple[dict[str, CoverageRow], list[str]]:
    rows: dict[str, CoverageRow] = {}
    errors: list[str] = []
    for match in COVERAGE_ROW_RE.finditer(text):
        row = CoverageRow(
            tag=match.group("tag"),
            date=match.group("date"),
            range_text=match.group("range").strip(),
            commits=int(match.group("commits")),
            status=match.group("status").strip(),
        )
        if row.tag in rows:
            errors.append(f"duplicate coverage row for {row.tag}")
        rows[row.tag] = row
    return rows, errors


def parse_sections(text: str) -> tuple[dict[str, DraftSection], list[str]]:
    wrappers = list(WRAPPER_RE.finditer(text))
    sections: dict[str, DraftSection] = {}
    errors: list[str] = []
    for idx, wrapper in enumerate(wrappers):
        wrapper_tag = wrapper.group("tag")
        block_start = wrapper.start()
        block_end = wrappers[idx + 1].start() if idx + 1 < len(wrappers) else len(text)
        block = text[block_start:block_end]
        range_text = parse_range_line(block)
        marker = SECTION_MARKER_RE.search(block)
        marker_status = normalize_marker_status(marker.group(1)) if marker is not None else None
        header = SECTION_HEADER_RE.search(block)
        header_tag = header.group("tag") if header is not None else None
        date = header.group("date") if header is not None else None
        body = ""
        if header is not None:
            body_region = block[header.end() :]
            next_top_level = re.search(r"^## ", body_region, re.MULTILINE)
            if next_top_level is not None:
                body_region = body_region[: next_top_level.start()]
            body = body_region.strip()
        if wrapper_tag in sections:
            errors.append(f"duplicate draft section wrapper for {wrapper_tag}")
        sections[wrapper_tag] = DraftSection(
            wrapper_tag=wrapper_tag,
            range_text=range_text,
            marker_status=marker_status,
            header_tag=header_tag,
            date=date,
            body=body,
        )
    return sections, errors


def parse_range_line(block: str) -> str | None:
    match = re.search(r"^Range: `([^`]+)`\s*$", block, re.MULTILINE)
    if match is None:
        return None
    return match.group(1)


def normalize_marker_status(marker: str) -> str:
    if marker == "Draft suggestion — human review required":
        return "pending human review"
    if marker.startswith("Human-reviewed by "):
        return "human-reviewed by " + marker.removeprefix("Human-reviewed by ")
    return marker


def validate(
    text: str,
    *,
    expected_tags_arg: str | None = None,
    require_reviewed: bool = False,
    check_git_ranges: bool = False,
) -> list[str]:
    errors: list[str] = []
    try:
        expected_tags = expected_tags_for_packet(text, expected_tags_arg)
    except (ValueError, subprocess.CalledProcessError) as exc:
        return [str(exc)]

    coverage, coverage_errors = parse_coverage(text)
    sections, section_errors = parse_sections(text)
    errors.extend(coverage_errors)
    errors.extend(section_errors)

    expected_set = set(expected_tags)
    coverage_set = set(coverage)
    section_set = set(sections)

    for tag in expected_tags:
        if tag not in coverage:
            errors.append(f"missing coverage row for {tag}")
        if tag not in sections:
            errors.append(f"missing draft section for {tag}")
    for tag in sorted(coverage_set - expected_set, key=version_number):
        errors.append(f"unexpected coverage row for {tag}")
    for tag in sorted(section_set - expected_set, key=version_number):
        errors.append(f"unexpected draft section for {tag}")

    for tag in expected_tags:
        row = coverage.get(tag)
        section = sections.get(tag)
        if row is None or section is None:
            continue
        validate_review_status(row, require_reviewed, errors)
        if section.marker_status != row.status:
            errors.append(
                f"{tag} section marker/status mismatch: coverage has {row.status!r}, "
                f"section marker has {section.marker_status!r}"
            )
        if section.range_text != row.range_text:
            errors.append(
                f"{tag} range mismatch: coverage has {row.range_text!r}, "
                f"section has {section.range_text!r}"
            )
        if section.header_tag != tag:
            errors.append(f"{tag} section header tag mismatch: {section.header_tag!r}")
        if section.date != row.date:
            errors.append(
                f"{tag} section date mismatch: coverage has {row.date!r}, "
                f"header has {section.date!r}"
            )
        if not section.body:
            errors.append(f"{tag} section body is empty")
            continue
        if CATEGORY_RE.search(section.body) is None:
            errors.append(f"{tag} section has no Keep-a-Changelog category heading")
        if not re.search(r"^- ", section.body, re.MULTILINE):
            errors.append(f"{tag} section has no bullet lines")
        if check_git_ranges:
            validate_git_range(row, errors)
    return errors


def validate_review_status(
    row: CoverageRow,
    require_reviewed: bool,
    errors: list[str],
) -> None:
    if row.status == "pending human review":
        if require_reviewed:
            errors.append(f"{row.tag} is still pending human review")
        return
    if REVIEWED_RE.match(row.status) is None:
        errors.append(
            f"{row.tag} review status must be 'pending human review' or "
            "'human-reviewed by <name> on YYYY-MM-DD'"
        )


def validate_git_range(row: CoverageRow, errors: list[str]) -> None:
    try:
        actual_date = git_tag_date(row.tag)
        actual_commits = git_no_merge_count(row.range_text)
    except subprocess.CalledProcessError as exc:
        stderr = exc.stderr.strip() if exc.stderr else str(exc)
        errors.append(f"{row.tag} git range check failed: {stderr}")
        return
    if actual_date != row.date:
        errors.append(
            f"{row.tag} date mismatch: coverage has {row.date!r}, git has {actual_date!r}"
        )
    if actual_commits != row.commits:
        errors.append(
            f"{row.tag} commit count mismatch: coverage has {row.commits}, "
            f"git has {actual_commits}"
        )


def git_tag_date(tag: str) -> str:
    result = subprocess.run(
        ["git", "log", "-1", "--format=%cs", tag],
        check=True,
        text=True,
        capture_output=True,
    )
    return result.stdout.strip()


def git_no_merge_count(range_text: str) -> int:
    result = subprocess.run(
        ["git", "rev-list", "--count", "--no-merges", range_text],
        check=True,
        text=True,
        capture_output=True,
    )
    return int(result.stdout.strip())


def main() -> int:
    args = parse_args()
    text = args.review.read_text(encoding="utf-8")
    errors = validate(
        text,
        expected_tags_arg=args.expected_tags,
        require_reviewed=args.require_reviewed,
        check_git_ranges=args.check_git_ranges,
    )
    if errors:
        for error in errors:
            print(f"::error::{error}", file=sys.stderr)
        return 1
    print(f"verified {args.review}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
