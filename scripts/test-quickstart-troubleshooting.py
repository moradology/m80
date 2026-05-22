#!/usr/bin/env python3
"""Tests for scripts/verify-quickstart-troubleshooting.py."""

from __future__ import annotations

import copy
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

from quickstart_snippets import public_command_inventory


REPO_ROOT = Path(__file__).resolve().parents[1]
VERIFY = REPO_ROOT / "scripts" / "verify-quickstart-troubleshooting.py"
RENDER = REPO_ROOT / "scripts" / "render-quickstart-troubleshooting.py"
COVERAGE = REPO_ROOT / "scripts" / "write-quickstart-troubleshooting-coverage.py"
MATRIX = REPO_ROOT / "docs" / "behaviors" / "release" / "quickstart-troubleshooting-matrix.json"
DOC = REPO_ROOT / "docs" / "behaviors" / "release" / "quickstart-troubleshooting-matrix.md"
COVERAGE_REPORT = REPO_ROOT / "docs" / "behaviors" / "release" / "quickstart-troubleshooting-coverage.json"


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

    def test_matrix_commands_reject_public_url_contract_drift(self) -> None:
        matrix = valid_matrix()
        row_by_id(matrix, "network")["likely_failing_command"] = (
            "curl -fsSL https://github.com/example/m80/releases/latest/download/install.sh | sudo sh"
        )

        result = run_verify(write_matrix(matrix))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("expected moradology/m80", result.stderr)

    def test_rendered_doc_matches_matrix(self) -> None:
        result = run_render()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, DOC.read_text())
        self.assertIn("| ID | Symptom | Likely failing command |", result.stdout)

    def test_rendered_doc_has_stable_anchor_for_each_id(self) -> None:
        rendered = run_render().stdout

        for row in valid_matrix()["rows"]:
            self.assertIn(f'id="{row["id"]}"', rendered)

    def test_render_check_rejects_stale_doc(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            stale_doc = Path(tmp) / "matrix.md"
            stale_doc.write_text("stale\n")

            result = subprocess.run(
                ["python3", str(RENDER), "--check", "--doc", str(stale_doc)],
                cwd=REPO_ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("quickstart troubleshooting doc is stale", result.stderr)

    def test_readme_runbook_and_command_inventory_see_matrix(self) -> None:
        readme = (REPO_ROOT / "README.md").read_text()
        runbook = (REPO_ROOT / "docs" / "runbook" / "release.md").read_text()

        for text in [readme, runbook]:
            self.assertIn("quickstart-troubleshooting-matrix.md#network", text)
            self.assertIn("quickstart-troubleshooting-matrix.md#process-smoke-failed", text)

        inventory = public_command_inventory(REPO_ROOT)
        self.assertTrue(inventory)

    def test_coverage_report_matches_matrix(self) -> None:
        result = run_coverage()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, COVERAGE_REPORT.read_text())
        report = json.loads(result.stdout)
        self.assertEqual(report["schema_version"], 1)
        self.assertEqual(
            set(report["required_lanes"]),
            {
                "installer_failure",
                "bootstrap_network_failure",
                "checksum_or_provenance_failure",
                "host_prerequisite_failure",
                "stale_profile_failure",
                "process_wrapper_smoke",
            },
        )
        self.assertEqual(
            report["manual_or_real_kvm_lanes"][0]["proof_artifact"],
            "docs/behaviors/release/public-install-proof-cache-v0.2.11.json",
        )

    def test_coverage_check_rejects_stale_report(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            stale_report = Path(tmp) / "coverage.json"
            stale_report.write_text("{}\n")

            result = subprocess.run(
                ["python3", str(COVERAGE), "--check", "--report", str(stale_report)],
                cwd=REPO_ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("quickstart troubleshooting coverage report is stale", result.stderr)

    def test_process_smoke_coverage_requires_real_kvm_proof(self) -> None:
        matrix = valid_matrix()
        process_row = row_by_id(matrix, "process-smoke-failed")
        process_row["source_mappings"] = [
            mapping for mapping in process_row["source_mappings"] if mapping["kind"] != "proof"
        ]

        result = run_coverage(write_matrix(matrix))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("process-smoke coverage must cite a proof source mapping", result.stderr)

    def test_process_smoke_coverage_requires_zero_exit_proof(self) -> None:
        matrix = valid_matrix()
        proof = json.loads(
            (
                REPO_ROOT
                / "docs"
                / "behaviors"
                / "release"
                / "public-install-proof-cache-v0.2.11.json"
            ).read_text()
        )
        proof["process_smoke"]["exit_code"] = 1
        with tempfile.TemporaryDirectory() as tmp:
            proof_path = Path(tmp) / "nonzero-proof.json"
            proof_path.write_text(json.dumps(proof))
            process_row = row_by_id(matrix, "process-smoke-failed")
            for mapping in process_row["source_mappings"]:
                if mapping["kind"] == "proof":
                    mapping["ref"] = str(proof_path)

            result = run_coverage(write_matrix(matrix))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("process_smoke.exit_code == 0", result.stderr)


def run_verify(matrix: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["python3", str(VERIFY), "--matrix", str(matrix)],
        cwd=REPO_ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def run_render() -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["python3", str(RENDER)],
        cwd=REPO_ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def run_coverage(matrix: Path = MATRIX) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["python3", str(COVERAGE), "--matrix", str(matrix)],
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
