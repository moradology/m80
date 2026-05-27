#!/usr/bin/env python3
"""Tests for scripts/verify-backfill-review.py."""

from __future__ import annotations

from pathlib import Path
import subprocess
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "verify-backfill-review.py"


VALID_PACKET = """# v0.2 Backfill Review Packet

Required stable tag range: v0.2.0 through v0.2.1 (captured at leaf claim time).

## Coverage Table

| Tag | Date | Range | Commits | Review status |
|---|---:|---|---:|---|
| v0.2.0 | 2026-05-06 | v0.1.0-smoke-passing..v0.2.0 | 65 | pending human review |
| v0.2.1 | 2026-05-06 | v0.2.0..v0.2.1 | 1 | pending human review |

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

## [v0.2.1] — 2026-05-06

v0.2.1 fixes a release workflow issue.

### Fixed

- Fixed release workflow target setup.
"""


class VerifyBackfillReviewTest(unittest.TestCase):
    def test_valid_pending_packet_passes_structural_check(self) -> None:
        result = run_verify(VALID_PACKET)

        self.assertEqual(result.returncode, 0)
        self.assertIn("verified ", result.stdout)

    def test_missing_section_fails(self) -> None:
        packet = VALID_PACKET.replace("### v0.2.1\n\nRange:", "### v0.2.9\n\nRange:")

        result = run_verify(packet, check=False)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("::error::missing draft section for v0.2.1", result.stderr)
        self.assertIn("::error::unexpected draft section for v0.2.9", result.stderr)

    def test_section_date_must_match_coverage_row(self) -> None:
        packet = VALID_PACKET.replace("## [v0.2.1] — 2026-05-06", "## [v0.2.1] — 2026-05-07")

        result = run_verify(packet, check=False)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            "::error::v0.2.1 section date mismatch: coverage has '2026-05-06', "
            "header has '2026-05-07'",
            result.stderr,
        )

    def test_require_reviewed_fails_pending_status(self) -> None:
        result = run_verify(VALID_PACKET, require_reviewed=True, check=False)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("::error::v0.2.0 is still pending human review", result.stderr)
        self.assertIn("::error::v0.2.1 is still pending human review", result.stderr)

    def test_require_reviewed_accepts_explicit_human_status(self) -> None:
        packet = VALID_PACKET.replace(
            "pending human review",
            "human-reviewed by release operator on 2026-05-27",
        )

        result = run_verify(packet, require_reviewed=True)

        self.assertEqual(result.returncode, 0)

    def test_body_needs_changelog_category_and_bullet(self) -> None:
        packet = VALID_PACKET.replace(
            "### Fixed\n\n- Fixed release workflow target setup.",
            "Release workflow target setup was fixed.",
        )

        result = run_verify(packet, check=False)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            "::error::v0.2.1 section has no Keep-a-Changelog category heading",
            result.stderr,
        )
        self.assertIn("::error::v0.2.1 section has no bullet lines", result.stderr)


def run_verify(
    packet: str,
    *,
    require_reviewed: bool = False,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    with tempfile.TemporaryDirectory() as tmp:
        review = Path(tmp) / "backfill-review.md"
        review.write_text(packet, encoding="utf-8")
        cmd = [
            "python3",
            str(SCRIPT),
            "--review",
            str(review),
            "--expected-tags",
            "v0.2.0,v0.2.1",
        ]
        if require_reviewed:
            cmd.append("--require-reviewed")
        return subprocess.run(cmd, check=check, text=True, capture_output=True)


if __name__ == "__main__":
    unittest.main()
