#!/usr/bin/env python3
"""Tests for current_latest_repair_preflight.py."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import tempfile
import unittest

from current_latest_repair_preflight import evaluate_preflight


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "current_latest_repair_preflight.py"
SOURCE_COMMIT = "0123456789abcdef0123456789abcdef01234567"


class CurrentLatestRepairPreflightTest(unittest.TestCase):
    def test_accepts_clean_new_stable_tag_that_supersedes_current_latest(self) -> None:
        artifact = accepted_fixture()

        self.assertTrue(artifact["decision"]["ok"])
        self.assertEqual(artifact["target_tag"], "v0.2.20")
        self.assertEqual(artifact["expected_release_tag"], "v0.2.20")
        self.assertFalse(artifact["dirty_tree"]["dirty"])
        self.assertEqual(artifact["existing_latest"]["tag"], "v0.2.11")
        self.assertFalse(artifact["supersedes_missing_installer_latest_state"])
        self.assertTrue(artifact["release_order"]["candidate_is_newer_than_existing_latest"])

    def test_rejects_dirty_tree_with_safe_repair_action(self) -> None:
        artifact = accepted_fixture(dirty_entries=[" M README.md"])

        diagnostic = only_diagnostic(artifact)
        self.assertFalse(artifact["decision"]["ok"])
        self.assertEqual(diagnostic["field"], "dirty_tree")
        self.assertEqual(diagnostic["expected"], "clean")
        self.assertIn("commit or discard dirty workspace changes", diagnostic["safe_repair_action"])

    def test_rejects_existing_tag_at_different_commit(self) -> None:
        artifact = accepted_fixture(tag_commit="fedcba9876543210fedcba9876543210fedcba98")

        diagnostic = only_diagnostic(artifact)
        self.assertEqual(diagnostic["field"], "tag_commit")
        self.assertEqual(diagnostic["expected"], SOURCE_COMMIT)
        self.assertIn("manual release-state repair", diagnostic["safe_repair_action"])

    def test_rejects_tag_version_mismatch(self) -> None:
        artifact = accepted_fixture(release_tag="v0.2.14")

        diagnostic = only_diagnostic(artifact)
        self.assertEqual(diagnostic["field"], "target_tag")
        self.assertEqual(diagnostic["expected"], "v0.2.20")
        self.assertEqual(diagnostic["observed"], "v0.2.14")
        self.assertIn("Cargo.toml workspace.package.version", diagnostic["safe_repair_action"])

    def test_rejects_attempted_old_release_backfill(self) -> None:
        artifact = accepted_fixture(
            release_tag="v0.2.7",
            workspace_package_version="0.2.7",
            tag_commit=SOURCE_COMMIT,
        )

        diagnostic = only_diagnostic(artifact)
        self.assertEqual(diagnostic["field"], "release_order")
        self.assertEqual(diagnostic["expected"], ">v0.2.11")
        self.assertIn("do not backfill old release assets", diagnostic["safe_repair_action"])
        self.assertFalse(artifact["supersedes_missing_installer_latest_state"])

    def test_rejects_missing_existing_latest_metadata(self) -> None:
        artifact = accepted_fixture(existing_latest_tag=None, existing_latest_url=None)

        diagnostic = only_diagnostic(artifact)
        self.assertEqual(diagnostic["field"], "existing_latest_tag")
        self.assertEqual(diagnostic["observed"], "missing")
        self.assertIn("unknown release state", diagnostic["safe_repair_action"])

    def test_rejects_unstable_existing_latest_metadata(self) -> None:
        artifact = accepted_fixture(existing_latest_tag="nightly")

        diagnostic = only_diagnostic(artifact)
        self.assertEqual(diagnostic["field"], "existing_latest_tag")
        self.assertEqual(diagnostic["expected"], "stable tag vMAJOR.MINOR.PATCH")
        self.assertIn("manual release-state repair", diagnostic["safe_repair_action"])

    def test_cli_accepts_current_workspace_version_without_override(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "preflight.json"
            result = subprocess.run(
                [
                    "python3",
                    str(SCRIPT),
                    "--release-tag",
                    "v0.2.20",
                    "--source-commit",
                    SOURCE_COMMIT,
                    "--tag-commit",
                    SOURCE_COMMIT,
                    "--existing-latest-tag",
                    "v0.2.11",
                    "--existing-latest-url",
                    "https://github.com/moradology/m80/releases/tag/v0.2.11",
                    "--dirty-status",
                    "clean",
                    "--generated-at",
                    "2026-05-21T00:00:00Z",
                    "--out",
                    str(out),
                ],
                cwd=REPO_ROOT,
                text=True,
                capture_output=True,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            payload = json.loads(out.read_text())
        self.assertEqual(payload["workspace_package_version"], "0.2.20")
        self.assertEqual(payload["expected_release_tag"], "v0.2.20")
        self.assertEqual(payload["decision"]["status"], "accepted")

    def test_cli_writes_rejected_artifact_before_exiting_nonzero(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "preflight.json"
            result = subprocess.run(
                [
                    "python3",
                    str(SCRIPT),
                    "--release-tag",
                    "v0.2.20",
                    "--workspace-version",
                    "0.2.20",
                    "--source-commit",
                    SOURCE_COMMIT,
                    "--tag-commit",
                    SOURCE_COMMIT,
                    "--existing-latest-tag",
                    "v0.2.11",
                    "--dirty-status",
                    "dirty",
                    "--dirty-entry",
                    " M Cargo.toml",
                    "--generated-at",
                    "2026-05-21T00:00:00Z",
                    "--out",
                    str(out),
                ],
                cwd=REPO_ROOT,
                text=True,
                capture_output=True,
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("field=dirty_tree", result.stderr)
            payload = json.loads(out.read_text())
        self.assertEqual(payload["decision"]["status"], "rejected")
        self.assertEqual(payload["dirty_tree"]["entries"], [" M Cargo.toml"])


def accepted_fixture(**overrides: object) -> dict:
    values = {
        "release_tag": "v0.2.20",
        "source_commit": SOURCE_COMMIT,
        "workspace_package_version": "0.2.20",
        "tag_commit": SOURCE_COMMIT,
        "existing_latest_tag": "v0.2.11",
        "existing_latest_url": "https://github.com/moradology/m80/releases/tag/v0.2.11",
        "missing_installer_latest_tag": "v0.2.7",
        "dirty_entries": [],
        "generated_at": "2026-05-21T00:00:00Z",
    }
    values.update(overrides)
    return evaluate_preflight(**values)  # type: ignore[arg-type]


def only_diagnostic(artifact: dict) -> dict:
    diagnostics = artifact["decision"]["diagnostics"]
    assert len(diagnostics) == 1, diagnostics
    return diagnostics[0]


if __name__ == "__main__":
    unittest.main()
