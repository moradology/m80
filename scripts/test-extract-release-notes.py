#!/usr/bin/env python3
"""Tests for scripts/extract-release-notes.py."""

from __future__ import annotations

from pathlib import Path
import subprocess
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "extract-release-notes.py"


CHANGELOG_FIXTURE = """# Changelog

## [Unreleased]

### Changed
- Future work.

## [v0.2.26] — 2026-05-27

v0.2.26 ships reviewed release notes.

### Changed
- Publish GitHub Release bodies from reviewed CHANGELOG.md sections.

### Fixed
- Fail release creation when reviewed release notes are missing.

## [v0.2.25] — 2026-05-27

v0.2.25 remains historical backfill scope.

### Changed
- Keep fallback historical release notes unchanged.
"""


class ExtractReleaseNotesTest(unittest.TestCase):
    def test_extracts_matching_section_body_and_is_idempotent(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            changelog = root / "CHANGELOG.md"
            first = root / "notes-first.md"
            second = root / "notes-second.md"
            changelog.write_text(CHANGELOG_FIXTURE, encoding="utf-8")

            first_run = run_extract(changelog, "v0.2.26", first, print_notes=True)
            second_run = run_extract(changelog, "v0.2.26", second, print_notes=True)

            expected = (
                "v0.2.26 ships reviewed release notes.\n\n"
                "### Changed\n"
                "- Publish GitHub Release bodies from reviewed CHANGELOG.md sections.\n\n"
                "### Fixed\n"
                "- Fail release creation when reviewed release notes are missing.\n"
            )
            self.assertEqual(first.read_text(encoding="utf-8"), expected)
            self.assertEqual(second.read_text(encoding="utf-8"), expected)
            self.assertEqual(first_run.stdout, expected)
            self.assertEqual(second_run.stdout, expected)

    def test_missing_section_fails_before_writing_notes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            changelog = root / "CHANGELOG.md"
            notes = root / "notes.md"
            changelog.write_text(CHANGELOG_FIXTURE, encoding="utf-8")

            result = run_extract(changelog, "v0.2.27", notes, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(notes.exists())
            self.assertIn(
                "::error::CHANGELOG.md has no reviewed release section for v0.2.27",
                result.stderr,
            )
            self.assertIn("Expected header: ## [v0.2.27] — YYYY-MM-DD", result.stderr)
            self.assertNotIn("release artifacts from", result.stderr)

    def test_header_without_date_is_not_a_release_section(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            changelog = root / "CHANGELOG.md"
            notes = root / "notes.md"
            changelog.write_text(
                "# Changelog\n\n## [v0.2.26]\n\n### Changed\n- Missing date.\n",
                encoding="utf-8",
            )

            result = run_extract(changelog, "v0.2.26", notes, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(notes.exists())


def run_extract(
    changelog: Path,
    tag: str,
    out: Path,
    *,
    print_notes: bool = False,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    cmd = [
        "python3",
        str(SCRIPT),
        "--changelog",
        str(changelog),
        "--tag",
        tag,
        "--out",
        str(out),
    ]
    if print_notes:
        cmd.append("--print")
    return subprocess.run(cmd, check=check, text=True, capture_output=True)


if __name__ == "__main__":
    unittest.main()
