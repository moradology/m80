#!/usr/bin/env python3
"""Tests for scripts/verify-quickstart-troubleshooting.py."""

from __future__ import annotations

import copy
import json
from pathlib import Path
import subprocess
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
VERIFY = REPO_ROOT / "scripts" / "verify-quickstart-troubleshooting.py"
MATRIX = REPO_ROOT / "docs" / "behaviors" / "release" / "quickstart-troubleshooting-matrix.json"


class QuickstartTroubleshootingTests(unittest.TestCase):
    def test_repo_matrix_passes(self) -> None:
        result = run_verify(MATRIX)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("quickstart troubleshooting matrix ok", result.stdout)

    def test_duplicate_ids_fail(self) -> None:
        matrix = valid_matrix()
        matrix["rows"].append(copy.deepcopy(matrix["rows"][0]))

        result = run_verify(write_matrix(matrix))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("duplicate id", result.stderr)

    def test_missing_owner_action_fails(self) -> None:
        matrix = valid_matrix()
        row_by_id(matrix, "network")["owner_action"] = ""

        result = run_verify(write_matrix(matrix))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("owner_action must be a nonempty string", result.stderr)

    def test_missing_source_mapping_fails(self) -> None:
        matrix = valid_matrix()
        row_by_id(matrix, "missing-asset")["source_mappings"] = []

        result = run_verify(write_matrix(matrix))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("source_mappings must be a nonempty list", result.stderr)

    def test_missing_required_id_fails(self) -> None:
        matrix = valid_matrix()
        matrix["rows"] = [row for row in matrix["rows"] if row["id"] != "kvm-unavailable"]

        result = run_verify(write_matrix(matrix))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing required troubleshooting id: kvm-unavailable", result.stderr)

    def test_undocumented_verifier_code_fails(self) -> None:
        matrix = valid_matrix()
        row_by_id(matrix, "unsupported-tuple")["verifier_codes"]["asset_index"].remove(
            "unsupported_host_tuple"
        )

        result = run_verify(write_matrix(matrix))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("undocumented asset_index verifier code: unsupported_host_tuple", result.stderr)

    def test_unknown_verifier_code_fails(self) -> None:
        matrix = valid_matrix()
        row_by_id(matrix, "network")["verifier_codes"]["asset_index"].append("mystery_code")

        result = run_verify(write_matrix(matrix))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unknown asset_index verifier code in matrix: mystery_code", result.stderr)

    def test_report_bug_rows_reject_install_repair_command(self) -> None:
        matrix = valid_matrix()
        row_by_id(matrix, "missing-asset")["repair_command"] = (
            "curl -fsSL https://github.com/moradology/m80/releases/download/v0.0.0/install.sh | sudo sh"
        )

        result = run_verify(write_matrix(matrix))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("report-release-bug rows must not use an install command", result.stderr)


def run_verify(matrix: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["python3", str(VERIFY), "--matrix", str(matrix)],
        cwd=REPO_ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def write_matrix(matrix: dict) -> Path:
    root = tempfile.TemporaryDirectory()
    path = Path(root.name) / "matrix.json"
    path.write_text(json.dumps(matrix))
    CLEANUPS.append(root)
    return path


def valid_matrix() -> dict:
    return json.loads(MATRIX.read_text())


def row_by_id(matrix: dict, row_id: str) -> dict:
    for row in matrix["rows"]:
        if row["id"] == row_id:
            return row
    raise AssertionError(f"missing row {row_id}")


CLEANUPS: list[tempfile.TemporaryDirectory] = []


if __name__ == "__main__":
    try:
        unittest.main()
    finally:
        for cleanup in CLEANUPS:
            cleanup.cleanup()
