#!/usr/bin/env python3
"""Tests for scripts/check-changelog-entry.py."""

from __future__ import annotations

from pathlib import Path
import subprocess
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "check-changelog-entry.py"


class CheckChangelogEntryTest(unittest.TestCase):
    def test_pull_request_without_changelog_emits_warning_and_succeeds(self) -> None:
        with fixture_repo() as repo:
            base = commit_file(repo, "README.md", "base\n")
            commit_file(repo, "src/lib.rs", "pub fn value() -> u8 { 1 }\n")

            result = run_check(repo, "pull_request", base)

            self.assertEqual(result.returncode, 0)
            self.assertIn("::warning title=CHANGELOG.md not changed::", result.stdout)
            self.assertIn("human-reviewed release-note text", result.stdout)

    def test_pull_request_with_changelog_change_has_no_warning(self) -> None:
        with fixture_repo() as repo:
            base = commit_file(repo, "README.md", "base\n")
            commit_file(repo, "CHANGELOG.md", "# Changelog\n\n## [Unreleased]\n\n- Entry.\n")

            result = run_check(repo, "pull_request", base)

            self.assertEqual(result.returncode, 0)
            self.assertIn("CHANGELOG.md changed", result.stdout)
            self.assertNotIn("::warning", result.stdout)

    def test_push_event_is_skipped(self) -> None:
        with fixture_repo() as repo:
            base = commit_file(repo, "README.md", "base\n")
            commit_file(repo, "src/lib.rs", "pub fn value() -> u8 { 1 }\n")

            result = run_check(repo, "push", base)

            self.assertEqual(result.returncode, 0)
            self.assertIn("skipped for event push", result.stdout)
            self.assertNotIn("::warning", result.stdout)


class fixture_repo:
    def __enter__(self) -> Path:
        self.tmp = tempfile.TemporaryDirectory()
        self.path = Path(self.tmp.name)
        subprocess.run(["git", "-C", str(self.path), "init"], check=True, capture_output=True)
        subprocess.run(
            ["git", "-C", str(self.path), "config", "user.email", "test@example.invalid"],
            check=True,
        )
        subprocess.run(
            ["git", "-C", str(self.path), "config", "user.name", "Test User"],
            check=True,
        )
        return self.path

    def __exit__(self, exc_type: object, exc: object, tb: object) -> None:
        self.tmp.cleanup()


def commit_file(repo: Path, relative: str, content: str) -> str:
    path = repo / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")
    subprocess.run(["git", "-C", str(repo), "add", relative], check=True)
    subprocess.run(["git", "-C", str(repo), "commit", "-m", f"update {relative}"], check=True, capture_output=True)
    return subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()


def run_check(repo: Path, event_name: str, base: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            "python3",
            str(SCRIPT),
            "--event-name",
            event_name,
            "--base",
            base,
            "--head",
            "HEAD",
            "--repo",
            str(repo),
        ],
        check=False,
        text=True,
        capture_output=True,
    )


if __name__ == "__main__":
    unittest.main()
