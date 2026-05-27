#!/usr/bin/env python3
"""Tests for scripts/apply-backfill-review.py."""

from __future__ import annotations

from pathlib import Path
import subprocess
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "apply-backfill-review.py"


REVIEW_PACKET = """# v0.2 Backfill Review Packet

Required stable tag range: v0.2.0 through v0.2.1 (captured at leaf claim time).

## Coverage Table

| Tag | Date | Range | Commits | Review status |
|---|---:|---|---:|---|
| v0.2.0 | 2026-05-06 | v0.1.0-smoke-passing..v0.2.0 | 65 | human-reviewed by release operator on 2026-05-27 |
| v0.2.1 | 2026-05-07 | v0.2.0..v0.2.1 | 1 | human-reviewed by release operator on 2026-05-27 |

## Draft Sections

### v0.2.0

Range: `v0.1.0-smoke-passing..v0.2.0`

Draft suggestion — human review required

## [v0.2.0] — 2026-05-06

v0.2.0 adds the first reviewed release-note shape.

### Added

- Added release notes for the first v0.2 tag.

### v0.2.1

Range: `v0.2.0..v0.2.1`

Draft suggestion — human review required

## [v0.2.1] — 2026-05-07

v0.2.1 fixes a release workflow issue.

### Fixed

- Fixed release workflow target setup.

## Next Action

Operator-only text that must not land in the final v0.2.1 section.
"""

CHANGELOG = """# Changelog

## [Unreleased]

### Added
- Old shipped material.

## [v0.1.0-smoke-passing] — 2026-05-04

### Added
- Earlier release.
"""


class ApplyBackfillReviewTest(unittest.TestCase):
    def test_applies_reviewed_sections_and_replaces_unreleased_body(self) -> None:
        with fixture_files(REVIEW_PACKET, CHANGELOG, "### Added\n- New unreleased work.\n") as fx:
            result = run_apply(fx)

            self.assertEqual(result.returncode, 0)
            changelog = fx.changelog.read_text(encoding="utf-8")
            self.assertIn("## [Unreleased]\n\n### Added\n- New unreleased work.", changelog)
            self.assertIn("## [v0.2.1] — 2026-05-07", changelog)
            self.assertLess(changelog.index("## [v0.2.1]"), changelog.index("## [v0.2.0]"))
            self.assertLess(changelog.index("## [v0.2.0]"), changelog.index("## [v0.1.0-smoke-passing]"))
            self.assertNotIn("Old shipped material", changelog)
            self.assertNotIn("Operator-only text", changelog)

    def test_pending_review_status_fails_before_writing(self) -> None:
        packet = REVIEW_PACKET.replace(
            "human-reviewed by release operator on 2026-05-27",
            "pending human review",
        )
        with fixture_files(packet, CHANGELOG, "### Added\n- New unreleased work.\n") as fx:
            before = fx.changelog.read_text(encoding="utf-8")

            result = run_apply(fx, check=False)

            self.assertEqual(result.returncode, 1)
            self.assertIn("::error::v0.2.0 is still pending human review", result.stderr)
            self.assertEqual(fx.changelog.read_text(encoding="utf-8"), before)

    def test_requires_explicit_unreleased_choice(self) -> None:
        with fixture_files(REVIEW_PACKET, CHANGELOG, "### Added\n- New unreleased work.\n") as fx:
            result = run_apply(fx, omit_unreleased=True, check=False)

            self.assertEqual(result.returncode, 2)
            self.assertIn(
                "::error::pass exactly one of --post-backfill-unreleased or --empty-unreleased",
                result.stderr,
            )

    def test_empty_unreleased_is_explicit(self) -> None:
        with fixture_files(REVIEW_PACKET, CHANGELOG, "unused\n") as fx:
            result = run_apply(fx, empty_unreleased=True)

            self.assertEqual(result.returncode, 0)
            self.assertIn(
                "## [Unreleased]\n\n## [v0.2.1]",
                fx.changelog.read_text(encoding="utf-8"),
            )

    def test_existing_v02_section_fails(self) -> None:
        changelog = CHANGELOG.replace(
            "## [v0.1.0-smoke-passing]",
            "## [v0.2.0] — 2026-05-06\n\n### Added\n- Duplicate.\n\n## [v0.1.0-smoke-passing]",
        )
        with fixture_files(REVIEW_PACKET, changelog, "### Added\n- New unreleased work.\n") as fx:
            result = run_apply(fx, check=False)

            self.assertEqual(result.returncode, 2)
            self.assertIn("already contains v0.2 backfill sections: v0.2.0", result.stderr)

    def test_dry_run_does_not_write(self) -> None:
        with fixture_files(REVIEW_PACKET, CHANGELOG, "### Added\n- New unreleased work.\n") as fx:
            before = fx.changelog.read_text(encoding="utf-8")

            result = run_apply(fx, dry_run=True)

            self.assertEqual(result.returncode, 0)
            self.assertIn("## [v0.2.1] — 2026-05-07", result.stdout)
            self.assertEqual(fx.changelog.read_text(encoding="utf-8"), before)


class fixture_files:
    def __init__(self, review: str, changelog: str, unreleased: str) -> None:
        self.review_text = review
        self.changelog_text = changelog
        self.unreleased_text = unreleased

    def __enter__(self) -> "fixture_files":
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.review = self.root / "backfill-review.md"
        self.changelog = self.root / "CHANGELOG.md"
        self.unreleased = self.root / "unreleased.md"
        self.review.write_text(self.review_text, encoding="utf-8")
        self.changelog.write_text(self.changelog_text, encoding="utf-8")
        self.unreleased.write_text(self.unreleased_text, encoding="utf-8")
        return self

    def __exit__(self, exc_type: object, exc: object, tb: object) -> None:
        self.tmp.cleanup()


def run_apply(
    fx: fixture_files,
    *,
    empty_unreleased: bool = False,
    omit_unreleased: bool = False,
    dry_run: bool = False,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    cmd = [
        "python3",
        str(SCRIPT),
        "--review",
        str(fx.review),
        "--changelog",
        str(fx.changelog),
        "--expected-tags",
        "v0.2.0,v0.2.1",
        "--skip-git-ranges",
    ]
    if not omit_unreleased:
        if empty_unreleased:
            cmd.append("--empty-unreleased")
        else:
            cmd.extend(["--post-backfill-unreleased", str(fx.unreleased)])
    if dry_run:
        cmd.append("--dry-run")
    return subprocess.run(cmd, check=check, text=True, capture_output=True)


if __name__ == "__main__":
    unittest.main()
