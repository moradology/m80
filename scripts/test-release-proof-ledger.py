#!/usr/bin/env python3
"""Tests for scripts/release_proof_ledger.py."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import tempfile
import unittest

import release_proof_ledger


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "release_proof_ledger.py"
LEDGER_NAME = "m80-release-proof-ledger.jsonl"


class ReleaseProofLedgerTest(unittest.TestCase):
    def test_append_and_verify_clean_chain(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            proof = write_proof(root, "m80-quickstart-proof-hostless.json")
            ledger = root / LEDGER_NAME

            run_ledger(
                "append",
                "--ledger",
                str(ledger),
                "--artifact-root",
                str(root),
                "--proof",
                str(proof),
                "--workflow-run-id",
                "12345",
                "--release-tag",
                "v0.0.0",
            )
            result = run_ledger(
                "verify",
                "--ledger",
                str(ledger),
                "--artifact-root",
                str(root),
                "--release-tag",
                "v0.0.0",
                "--require-proof",
                "m80-quickstart-proof-hostless.json",
                "--require-proof-type",
                "hostless",
                "--expect-record-count",
                "1",
            )

            self.assertIn("release proof ledger ok", result.stdout)
            record = read_records(ledger)[0]
            self.assertEqual(record["previous_record_hash"], None)
            self.assertEqual(record["proof_type"], "hostless")
            self.assertEqual(record["substrate"], "hostless")
            self.assertEqual(record["release_tag"], "v0.0.0")
            self.assertNotIn("/tmp/m80-secret-install-root", ledger.read_text())

    def test_verify_rejects_truncated_chain(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            ledger = root / LEDGER_NAME
            append_fixture(root, ledger, "hostless.json", proof_kind="hostless")
            append_fixture(root, ledger, "real-kvm.json", proof_kind="real-kvm")
            ledger.write_text(ledger.read_text().splitlines()[0] + "\n")

            result = run_ledger(
                "verify",
                "--ledger",
                str(ledger),
                "--artifact-root",
                str(root),
                "--expect-record-count",
                "2",
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("record count mismatch", result.stderr)

    def test_verify_rejects_reordered_records(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            ledger = root / LEDGER_NAME
            append_fixture(root, ledger, "hostless.json", proof_kind="hostless")
            append_fixture(root, ledger, "real-kvm.json", proof_kind="real-kvm")
            lines = ledger.read_text().splitlines()
            ledger.write_text("\n".join(reversed(lines)) + "\n")

            result = run_ledger("verify", "--ledger", str(ledger), "--artifact-root", str(root), check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("previous_record_hash mismatch", result.stderr)

    def test_verify_rejects_swapped_proof_path(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            ledger = root / LEDGER_NAME
            append_fixture(root, ledger, "hostless.json", proof_kind="hostless")
            write_proof(root, "replacement.json", proof_kind="hostless")
            record = read_records(ledger)[0]
            record["proof_artifact"] = "replacement.json"
            record["proof_artifact_digest"] = release_proof_ledger.sha256_ref(root / "replacement.json")
            record["record_hash"] = release_proof_ledger.record_hash(record)
            write_records(ledger, [record])

            result = run_ledger(
                "verify",
                "--ledger",
                str(ledger),
                "--artifact-root",
                str(root),
                "--require-proof",
                "hostless.json",
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("missing required proof artifact: hostless.json", result.stderr)

    def test_verify_rejects_stale_proof_digest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            ledger = root / LEDGER_NAME
            proof = append_fixture(root, ledger, "hostless.json", proof_kind="hostless")
            value = json.loads(proof.read_text())
            value["release"]["resolved_tag"] = "v0.0.1"
            proof.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")

            result = run_ledger("verify", "--ledger", str(ledger), "--artifact-root", str(root), check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("proof_artifact_digest mismatch", result.stderr)

    def test_verify_rejects_schema_version_drift(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            ledger = root / LEDGER_NAME
            append_fixture(root, ledger, "hostless.json", proof_kind="hostless")
            record = read_records(ledger)[0]
            record["schema_version"] = 2
            record["record_hash"] = release_proof_ledger.record_hash(record)
            write_records(ledger, [record])

            result = run_ledger("verify", "--ledger", str(ledger), "--artifact-root", str(root), check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("schema_version mismatch", result.stderr)

    def test_verify_rejects_redaction_drift(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            ledger = root / LEDGER_NAME
            append_fixture(root, ledger, "hostless.json", proof_kind="hostless")
            record = read_records(ledger)[0]
            record["redaction"]["host_paths"] = "/tmp/m80-secret-install-root"
            record["record_hash"] = release_proof_ledger.record_hash(record)
            write_records(ledger, [record])

            result = run_ledger("verify", "--ledger", str(ledger), "--artifact-root", str(root), check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("redaction host_paths must be omitted", result.stderr)


def append_fixture(root: Path, ledger: Path, name: str, *, proof_kind: str) -> Path:
    proof = write_proof(root, name, proof_kind=proof_kind)
    run_ledger(
        "append",
        "--ledger",
        str(ledger),
        "--artifact-root",
        str(root),
        "--proof",
        str(proof),
        "--workflow-run-id",
        "12345",
        "--release-tag",
        "v0.0.0",
    )
    return proof


def write_proof(root: Path, name: str, *, proof_kind: str = "hostless") -> Path:
    path = root / name
    payload = {
        "schema_version": 1,
        "proof_kind": proof_kind,
        "release": {
            "requested": "v0.0.0",
            "resolved_tag": "v0.0.0",
        },
        "install": {
            "root": "/tmp/m80-secret-install-root",
        },
        "substrate": {
            "kind": proof_kind,
            "summary": "fixture",
        },
    }
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    return path


def run_ledger(*args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(
        ["python3", str(SCRIPT), *args],
        cwd=REPO_ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if check and result.returncode != 0:
        raise AssertionError(
            f"release_proof_ledger.py failed with {result.returncode}\n"
            f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        )
    return result


def read_records(ledger: Path) -> list[dict]:
    return [json.loads(line) for line in ledger.read_text().splitlines() if line.strip()]


def write_records(ledger: Path, records: list[dict]) -> None:
    ledger.write_text(
        "".join(json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n" for record in records)
    )


if __name__ == "__main__":
    unittest.main()
