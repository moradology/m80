#!/usr/bin/env python3
"""Tests for scripts/verify-release-notes-proof.py."""

from __future__ import annotations

from pathlib import Path
import subprocess
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "verify-release-notes-proof.py"
COMMIT_SHA = "0123456789abcdef0123456789abcdef01234567"
WORKFLOW_URL = "https://github.com/moradology/m80/actions/runs/123456789"

BODY = """v1.2.3 ships the reviewed release-notes proof path. The body is intentionally
long enough to satisfy the C7 measurement bound and mirrors exactly what
CHANGELOG.md says for this release after human review.

### Added

- Added proof capture for GitHub Release notes so the release operator can
  compare the live body against committed changelog text before closing the
  release-notes pipeline.
"""

CHANGELOG = f"""# Changelog

## [Unreleased]

## [v1.2.3] — 2026-05-27

{BODY}

## [v1.2.2] — 2026-05-26

### Fixed

- Previous release.
"""


class ReleaseNotesProofTest(unittest.TestCase):
    def test_writes_proof_when_release_body_matches_changelog(self) -> None:
        with fixture_files(CHANGELOG, BODY) as fx:
            result = run_proof(fx)

            self.assertEqual(result.returncode, 0)
            proof = fx.out.read_text(encoding="utf-8")
            self.assertIn("# Release Notes Proof: v1.2.3", proof)
            self.assertIn(f"- source_commit: `{COMMIT_SHA}`", proof)
            self.assertIn(f"- workflow_run_url: {WORKFLOW_URL}", proof)
            self.assertIn("- human_reviewed_by: `release operator`", proof)
            self.assertIn("The GitHub Release body exactly matched", proof)
            self.assertIn(BODY.strip(), proof)

    def test_body_mismatch_fails_without_writing_proof(self) -> None:
        with fixture_files(CHANGELOG, BODY + "\nExtra remote-only text.\n") as fx:
            result = run_proof(fx, check=False)

            self.assertEqual(result.returncode, 1)
            self.assertIn(
                "::error::GitHub Release body does not exactly match the CHANGELOG.md section body",
                result.stderr,
            )
            self.assertFalse(fx.out.exists())

    def test_old_fallback_body_fails(self) -> None:
        with fixture_files(
            CHANGELOG,
            "m80 v1.2.3 release artifacts from https://github.com/moradology/m80/actions/runs/123\n",
        ) as fx:
            result = run_proof(fx, check=False)

            self.assertEqual(result.returncode, 1)
            self.assertIn("::error::GitHub Release body still matches the old one-line fallback", result.stderr)

    def test_missing_changelog_section_fails(self) -> None:
        with fixture_files("# Changelog\n\n## [Unreleased]\n", BODY) as fx:
            result = run_proof(fx, check=False)

            self.assertEqual(result.returncode, 1)
            self.assertIn(
                "::error::CHANGELOG.md has no reviewed release section for the target tag",
                result.stderr,
            )

    def test_invalid_workflow_run_url_fails_before_writing(self) -> None:
        with fixture_files(CHANGELOG, BODY) as fx:
            result = run_proof(fx, workflow_url="https://example.invalid/run/1", check=False)

            self.assertEqual(result.returncode, 2)
            self.assertIn("::error::--workflow-run-url must be a GitHub Actions run URL", result.stderr)
            self.assertFalse(fx.out.exists())

    def test_dry_run_prints_without_writing(self) -> None:
        with fixture_files(CHANGELOG, BODY) as fx:
            result = run_proof(fx, dry_run=True)

            self.assertEqual(result.returncode, 0)
            self.assertIn("# Release Notes Proof: v1.2.3", result.stdout)
            self.assertFalse(fx.out.exists())


class fixture_files:
    def __init__(self, changelog: str, body: str) -> None:
        self.changelog_text = changelog
        self.body_text = body

    def __enter__(self) -> "fixture_files":
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.changelog = self.root / "CHANGELOG.md"
        self.body = self.root / "body.md"
        self.out = self.root / "proof.md"
        self.changelog.write_text(self.changelog_text, encoding="utf-8")
        self.body.write_text(self.body_text, encoding="utf-8")
        return self

    def __exit__(self, exc_type: object, exc: object, tb: object) -> None:
        self.tmp.cleanup()


def run_proof(
    fx: fixture_files,
    *,
    workflow_url: str = WORKFLOW_URL,
    dry_run: bool = False,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    cmd = [
        "python3",
        str(SCRIPT),
        "--tag",
        "v1.2.3",
        "--changelog",
        str(fx.changelog),
        "--release-body-file",
        str(fx.body),
        "--workflow-run-url",
        workflow_url,
        "--commit-sha",
        COMMIT_SHA,
        "--human-reviewed-by",
        "release operator",
        "--out",
        str(fx.out),
    ]
    if dry_run:
        cmd.append("--dry-run")
    return subprocess.run(cmd, check=check, text=True, capture_output=True)


if __name__ == "__main__":
    unittest.main()
