#!/usr/bin/env python3
"""Tests for update-freshness-status-from-public-access.py."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

import freshness_status


REPO_ROOT = Path(__file__).resolve().parents[1]
UPDATE = REPO_ROOT / "scripts" / "update-freshness-status-from-public-access.py"
PUBLIC_ACCESS_TEST = REPO_ROOT / "scripts" / "test-release-public-access-receipt.py"


class UpdateFreshnessStatusFromPublicAccessTest(unittest.TestCase):
    def test_generates_valid_public_green_status(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            artifact_root = root / "docs" / "behaviors" / "release"
            artifact_root.mkdir(parents=True)
            receipt_path = artifact_root / "release-readiness-public-access.json"
            status_path = artifact_root / "freshness-status-docs.json"
            receipt_path.write_text(json.dumps(valid_receipt(), indent=2, sort_keys=True) + "\n")

            result = run_update(receipt_path, status_path)

            self.assertEqual(result.returncode, 0, result.stderr)
            status = json.loads(status_path.read_text())
            self.assertEqual(status["status"], "public_green")
            self.assertEqual(status["resolved_latest_tag"], "v0.2.7")
            self.assertEqual(status["workflow_run_id"], "26263525140")
            self.assertEqual(status["proof_artifacts"][0]["path"], "release-readiness-public-access.json")
            self.assertEqual(status["checked_command_inventory_digest"], freshness_status.command_inventory_digest(REPO_ROOT))
            self.assertEqual(len(status["public_assets"]), 16)

    def test_workflow_run_id_flag_covers_receipts_without_builder_identity(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            artifact_root = root / "docs" / "behaviors" / "release"
            artifact_root.mkdir(parents=True)
            receipt = valid_receipt()
            receipt["release_build"]["builder_identity"] = None
            receipt_path = artifact_root / "release-readiness-public-access.json"
            status_path = artifact_root / "freshness-status-docs.json"
            receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")

            result = run_update(receipt_path, status_path, "--workflow-run-id", "123")

            self.assertEqual(result.returncode, 0, result.stderr)
            status = json.loads(status_path.read_text())
            self.assertEqual(status["workflow_run_id"], "123")

    def test_missing_workflow_run_id_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            artifact_root = root / "docs" / "behaviors" / "release"
            artifact_root.mkdir(parents=True)
            receipt = valid_receipt()
            receipt["release_build"]["builder_identity"] = None
            receipt_path = artifact_root / "release-readiness-public-access.json"
            status_path = artifact_root / "freshness-status-docs.json"
            receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")

            result = run_update(receipt_path, status_path)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("missing --workflow-run-id", result.stderr)


def run_update(receipt_path: Path, status_path: Path, *extra: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            "python3",
            str(UPDATE),
            "--receipt",
            str(receipt_path),
            "--out",
            str(status_path),
            "--docs-root",
            str(REPO_ROOT),
            *extra,
        ],
        cwd=REPO_ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


def valid_receipt() -> dict:
    spec = importlib.util.spec_from_file_location("public_access_receipt_fixture", PUBLIC_ACCESS_TEST)
    if spec is None or spec.loader is None:
        raise RuntimeError("could not load public-access receipt fixture")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.valid_receipt()


if __name__ == "__main__":
    unittest.main()
