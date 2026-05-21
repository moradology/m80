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
VERIFY = REPO_ROOT / "scripts" / "verify-quickstart-proof.py"
LEDGER_NAME = "m80-release-proof-ledger.jsonl"


class ReleaseProofLedgerTest(unittest.TestCase):
    def test_append_and_verify_clean_chain(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            proof = write_proof(root, "m80-quickstart-proof-hostless.json")
            result_path = write_verifier_result(root, proof, "hostless.verifier-result.json")
            ledger = root / LEDGER_NAME

            run_ledger(
                "append",
                "--ledger",
                str(ledger),
                "--artifact-root",
                str(root),
                "--proof",
                str(proof),
                "--verifier-result",
                str(result_path),
                "--log-artifact",
                str(root / "stderr.txt"),
                "--workflow-run-id",
                "12345",
                "--runner-identity",
                "github-actions:fixture/run/12345/attempts/1",
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
            self.assertEqual(
                record["command"],
                {
                    "display": "m80 run -- echo hello",
                    "expected_exit_status": 0,
                    "observed_exit_status": 0,
                },
            )
            self.assertEqual(record["m80_version"], "v0.0.0")
            self.assertEqual(record["bundle_metadata"]["path"], "bundle.json")
            self.assertEqual(record["install"]["active_pointer"], "<redacted>/active")
            self.assertEqual(record["install"]["default_profile"], "<redacted>/default.toml")
            self.assertEqual(record["verifier_result"]["path"], "hostless.verifier-result.json")
            self.assertEqual(record["log_artifacts"][0]["path"], "stderr.txt")
            self.assertEqual(record["runner_identity"], "github-actions:fixture/run/12345/attempts/1")
            self.assertEqual(record["pass_fail_summary"], {"passed": True, "summary": "quickstart proof validated"})
            self.assertNotIn("/tmp/m80-secret-install-root", ledger.read_text())

    def test_append_rejects_missing_command(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            ledger = root / LEDGER_NAME
            proof = write_proof(root, "hostless.json", proof_kind="hostless")
            result_path = write_verifier_result(root, proof, "hostless.verifier-result.json")
            payload = json.loads(proof.read_text())
            del payload["command"]
            proof.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
            result_payload = json.loads(result_path.read_text())
            result_payload["proof_artifact_digest"] = release_proof_ledger.sha256_ref(proof)
            result_path.write_text(json.dumps(result_payload, indent=2, sort_keys=True) + "\n")

            result = run_ledger(
                "append",
                "--ledger",
                str(ledger),
                "--artifact-root",
                str(root),
                "--proof",
                str(proof),
                "--verifier-result",
                str(result_path),
                "--log-artifact",
                str(root / "stderr.txt"),
                "--workflow-run-id",
                "12345",
                "--runner-identity",
                "github-actions:fixture/run/12345/attempts/1",
                "--release-tag",
                "v0.0.0",
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("quickstart proof command must be an object", result.stderr)

    def test_verify_rejects_stale_bundle_metadata_hash(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            ledger = root / LEDGER_NAME
            append_fixture(root, ledger, "hostless.json", proof_kind="hostless")
            metadata = json.loads((root / "bundle.json").read_text())
            metadata["m80_version"] = "v0.0.1"
            (root / "bundle.json").write_text(json.dumps(metadata, indent=2, sort_keys=True) + "\n")

            result = run_ledger("verify", "--ledger", str(ledger), "--artifact-root", str(root), check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("bundle metadata hash mismatch", result.stderr)

    def test_verify_rejects_missing_verifier_result_path(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            ledger = root / LEDGER_NAME
            append_fixture(root, ledger, "hostless.json", proof_kind="hostless")
            (root / "hostless.verifier-result.json").unlink()

            result = run_ledger("verify", "--ledger", str(ledger), "--artifact-root", str(root), check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("verifier result missing", result.stderr)

    def test_append_rejects_missing_runner_identity(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            ledger = root / LEDGER_NAME
            proof = write_proof(root, "hostless.json", proof_kind="hostless")
            result_path = write_verifier_result(root, proof, "hostless.verifier-result.json")

            result = run_ledger(
                "append",
                "--ledger",
                str(ledger),
                "--artifact-root",
                str(root),
                "--proof",
                str(proof),
                "--verifier-result",
                str(result_path),
                "--log-artifact",
                str(root / "stderr.txt"),
                "--workflow-run-id",
                "12345",
                "--runner-identity",
                "",
                "--release-tag",
                "v0.0.0",
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("runner_identity must be a nonempty string", result.stderr)

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
            self.assertIn("quickstart verifier result proof_artifact mismatch", result.stderr)

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
            record["schema_version"] = 1
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
    result_path = write_verifier_result(root, proof, f"{Path(name).stem}.verifier-result.json")
    run_ledger(
        "append",
        "--ledger",
        str(ledger),
        "--artifact-root",
        str(root),
        "--proof",
        str(proof),
        "--verifier-result",
        str(result_path),
        "--log-artifact",
        str(root / "stderr.txt"),
        "--workflow-run-id",
        "12345",
        "--runner-identity",
        "github-actions:fixture/run/12345/attempts/1",
        "--release-tag",
        "v0.0.0",
    )
    return proof


def write_proof(root: Path, name: str, *, proof_kind: str = "hostless") -> Path:
    path = root / name
    write_json(
        root / "bundle.json",
        {
            "release_tag": "v0.0.0",
            "m80_version": "v0.0.0",
            "guest_protocol_version": 1,
            "manifest_schema_version": 5,
        },
    )
    write_json(root / "host-binaries.manifest.json", {"schema_version": 1})
    (root / "stderr.txt").write_text("")
    payload = {
        "schema_version": 1,
        "proof_kind": proof_kind,
        "release": {
            "requested": "v0.0.0",
            "resolved_tag": "v0.0.0",
            "install_url": "https://github.com/moradology/m80/releases/download/v0.0.0/install.sh",
        },
        "command": {
            "display": "m80 run -- echo hello",
            "argv": ["m80", "run", "--", "echo", "hello"],
            "expected_exit_status": 0,
            "observed_exit_status": 0,
            "expected_nonzero": False,
        },
        "stream_expectations": {
            "stdout_contains": "hello",
            "stderr_contains": "",
        },
        "stdout": {"excerpt": "hello\n"},
        "stderr": {"path": "stderr.txt"},
        "install": {
            "root": "/tmp/m80-secret-install-root",
            "active_pointer": "/tmp/m80-secret-install-root/active",
            "default_profile": "/tmp/m80-secret-install-root/profiles/default.toml",
        },
        "m80": {
            "version": "v0.0.0",
            "release_tag": "v0.0.0",
            "version_status": "release",
        },
        "bundle": {
            "metadata_path": "bundle.json",
            "release_tag": "v0.0.0",
            "m80_version": "v0.0.0",
            "guest_protocol_version": 1,
            "manifest_schema_version": 5,
        },
        "host_binaries": {
            "manifest_path": "host-binaries.manifest.json",
            "firecracker_version": "v1.15.1",
            "jailer_version": "v1.15.1",
        },
        "substrate": {
            "kind": proof_kind,
            "summary": "fixture",
        },
    }
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    return path


def write_verifier_result(root: Path, proof: Path, name: str) -> Path:
    result_path = root / name
    result = subprocess.run(
        [
            "python3",
            str(VERIFY),
            str(proof),
            "--artifact-root",
            str(root),
            "--release-tag",
            "v0.0.0",
            "--result-out",
            str(result_path),
        ],
        cwd=REPO_ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if result.returncode != 0:
        raise AssertionError(
            f"verify-quickstart-proof.py failed with {result.returncode}\n"
            f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        )
    return result_path


def write_json(path: Path, payload: dict) -> None:
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")


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
