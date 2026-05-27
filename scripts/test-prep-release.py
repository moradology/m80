#!/usr/bin/env python3
"""Tests for scripts/prep-release.sh."""

from __future__ import annotations

from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "prep-release.sh"
FIXTURES = REPO_ROOT / "tests" / "fixtures" / "prep-release"


class PrepReleaseTest(unittest.TestCase):
    def test_missing_version_exits_2(self) -> None:
        with fixture_repo("changelog-valid.md") as repo:
            result = run_prep(repo, "--date", "2026-05-27")

            self.assertEqual(result.returncode, 2)
            self.assertIn("missing required --version", result.stderr)

    def test_dirty_tree_exits_3(self) -> None:
        with fixture_repo("changelog-valid.md") as repo:
            (repo / "README.md").write_text("dirty\n", encoding="utf-8")

            result = run_prep(repo, "--version", "v1.2.3", "--date", "2026-05-27")

            self.assertEqual(result.returncode, 3)
            self.assertIn("requires a clean git tree", result.stderr)

    def test_existing_tag_exits_4(self) -> None:
        with fixture_repo("changelog-valid.md") as repo:
            subprocess.run(["git", "-C", str(repo), "tag", "v1.2.3"], check=True)

            result = run_prep(repo, "--version", "v1.2.3", "--date", "2026-05-27")

            self.assertEqual(result.returncode, 4)
            self.assertIn("target tag already exists", result.stderr)

    def test_missing_unreleased_exits_5(self) -> None:
        with fixture_repo("changelog-missing-unreleased.md") as repo:
            result = run_prep(repo, "--version", "v1.2.3", "--date", "2026-05-27")

            self.assertEqual(result.returncode, 5)
            self.assertIn("no ## [Unreleased] section", result.stderr)

    def test_empty_unreleased_exits_6_with_draft_hint(self) -> None:
        with fixture_repo("changelog-empty-unreleased.md") as repo:
            result = run_prep(repo, "--version", "v1.2.3", "--date", "2026-05-27")

            self.assertEqual(result.returncode, 6)
            self.assertIn("/draft-release-notes", result.stderr)
            self.assertIn("review/refine", result.stderr)

    def test_dry_run_prints_diff_without_writing(self) -> None:
        with fixture_repo("changelog-valid.md") as repo:
            before_changelog = (repo / "CHANGELOG.md").read_text(encoding="utf-8")
            before_cargo = (repo / "Cargo.toml").read_text(encoding="utf-8")

            result = run_prep(
                repo,
                "--version",
                "v1.2.3",
                "--date",
                "2026-05-27",
                "--dry-run",
            )

            self.assertEqual(result.returncode, 0)
            self.assertIn("prep-release dry run for v1.2.3", result.stdout)
            self.assertIn("## [v1.2.3] — 2026-05-27", result.stdout)
            self.assertIn('version = "1.2.3"', result.stdout)
            self.assertEqual((repo / "CHANGELOG.md").read_text(encoding="utf-8"), before_changelog)
            self.assertEqual((repo / "Cargo.toml").read_text(encoding="utf-8"), before_cargo)

    def test_success_promotes_unreleased_and_prints_next_steps(self) -> None:
        with fixture_repo("changelog-valid.md") as repo:
            result = run_prep(
                repo,
                "--version",
                "v1.2.3",
                "--date",
                "2026-05-27",
                "--skip-cargo-check",
            )

            self.assertEqual(result.returncode, 0)
            changelog = (repo / "CHANGELOG.md").read_text(encoding="utf-8")
            cargo = (repo / "Cargo.toml").read_text(encoding="utf-8")
            self.assertIn("## [Unreleased]\n\n## [v1.2.3] — 2026-05-27", changelog)
            self.assertIn("- Human-reviewed release note.", changelog)
            self.assertIn('version = "1.2.3"', cargo)
            self.assertIn('git commit -m "Release v1.2.3"', result.stdout)
            self.assertIn("git tag -a v1.2.3 -m \"v1.2.3\"", result.stdout)


class fixture_repo:
    def __init__(self, changelog_fixture: str) -> None:
        self.changelog_fixture = changelog_fixture

    def __enter__(self) -> Path:
        self.tmp = tempfile.TemporaryDirectory()
        self.path = Path(self.tmp.name)
        shutil.copy(FIXTURES / self.changelog_fixture, self.path / "CHANGELOG.md")
        shutil.copy(FIXTURES / "Cargo.toml", self.path / "Cargo.toml")
        subprocess.run(["git", "-C", str(self.path), "init"], check=True, capture_output=True)
        subprocess.run(
            ["git", "-C", str(self.path), "config", "user.email", "test@example.invalid"],
            check=True,
        )
        subprocess.run(
            ["git", "-C", str(self.path), "config", "user.name", "Test User"],
            check=True,
        )
        subprocess.run(["git", "-C", str(self.path), "add", "."], check=True)
        subprocess.run(["git", "-C", str(self.path), "commit", "-m", "fixture"], check=True, capture_output=True)
        return self.path

    def __exit__(self, exc_type: object, exc: object, tb: object) -> None:
        self.tmp.cleanup()


def run_prep(repo: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["bash", str(SCRIPT), "--repo", str(repo), *args],
        check=False,
        text=True,
        capture_output=True,
    )


if __name__ == "__main__":
    unittest.main()
