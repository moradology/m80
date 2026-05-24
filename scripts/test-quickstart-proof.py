#!/usr/bin/env python3
"""Unit tests for scripts/verify-quickstart-proof.py."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
VERIFY = REPO_ROOT / "scripts" / "verify-quickstart-proof.py"
WRITER = REPO_ROOT / "scripts" / "write-quickstart-proof-fixture.py"


class QuickstartProofTest(unittest.TestCase):
    def test_valid_hostless_fixture_proof_passes(self) -> None:
        with proof_fixture() as fixture:
            result = run_verify(fixture.proof, fixture.root, "v0.0.0")

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("validated quickstart proof", result.stdout)

    def test_result_out_records_validated_summary_without_host_paths(self) -> None:
        with proof_fixture() as fixture:
            result_path = fixture.root / "proof.verifier-result.json"
            result = run_verify(fixture.proof, fixture.root, "v0.0.0", result_out=result_path)

            self.assertEqual(result.returncode, 0, result.stderr)
            payload = read_json(result_path)
            self.assertEqual(payload["verifier"], "verify-quickstart-proof.py")
            self.assertEqual(payload["proof_artifact"], "proof.json")
            self.assertEqual(payload["release_tag"], "v0.0.0")
            self.assertTrue(payload["passed"])
            self.assertEqual(
                payload["command"],
                {
                    "display": "sudo m80 run -- echo hello",
                    "expected_exit_status": 0,
                    "observed_exit_status": 0,
                },
            )
            self.assertEqual(payload["m80_version"], "v0.0.0")
            self.assertEqual(payload["bundle_metadata"]["path"], "bundle.json")
            self.assertEqual(payload["install"]["active_pointer"], "<redacted>/active")
            self.assertEqual(payload["install"]["default_profile"], "<redacted>/default.toml")
            self.assertNotIn("/tmp/m80-install-root", result_path.read_text())

    def test_valid_real_kvm_proof_uses_same_schema(self) -> None:
        with proof_fixture(proof_kind="real-kvm") as fixture:
            result = run_verify(fixture.proof, fixture.root, "v0.0.0")

            self.assertEqual(result.returncode, 0, result.stderr)

    def test_missing_required_field_fails(self) -> None:
        with proof_fixture() as fixture:
            proof = read_json(fixture.proof)
            del proof["command"]
            write_json(fixture.proof, proof)

            result = run_verify(fixture.proof, fixture.root, "v0.0.0")

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("missing field(s): command", result.stderr)

    def test_stale_artifact_path_fails(self) -> None:
        with proof_fixture() as fixture:
            (fixture.root / "stderr.txt").unlink()

            result = run_verify(fixture.proof, fixture.root, "v0.0.0")

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("quickstart proof stderr path is missing", result.stderr)

    def test_mismatched_resolved_tag_fails(self) -> None:
        with proof_fixture() as fixture:
            result = run_verify(fixture.proof, fixture.root, "v9.9.9")

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("resolved tag mismatch", result.stderr)

    def test_expected_nonzero_exit_status_passes_when_observed_matches(self) -> None:
        with proof_fixture() as fixture:
            proof = read_json(fixture.proof)
            proof["command"]["expected_exit_status"] = 42
            proof["command"]["observed_exit_status"] = 42
            proof["command"]["expected_nonzero"] = True
            proof["stdout"]["excerpt"] = "stdout-marker\n"
            proof["stderr"] = {"excerpt": "stderr-marker\n"}
            proof["stream_expectations"] = {
                "stdout_contains": "stdout-marker",
                "stderr_contains": "stderr-marker",
            }
            write_json(fixture.proof, proof)

            result = run_verify(fixture.proof, fixture.root, "v0.0.0")

            self.assertEqual(result.returncode, 0, result.stderr)

    def test_missing_expected_exit_status_fails(self) -> None:
        with proof_fixture() as fixture:
            proof = read_json(fixture.proof)
            del proof["command"]["expected_exit_status"]
            write_json(fixture.proof, proof)

            result = run_verify(fixture.proof, fixture.root, "v0.0.0")

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("missing field(s): expected_exit_status", result.stderr)

    def test_expected_nonzero_rejects_unexpected_zero_exit(self) -> None:
        with proof_fixture() as fixture:
            proof = read_json(fixture.proof)
            proof["command"]["expected_exit_status"] = 42
            proof["command"]["observed_exit_status"] = 0
            proof["command"]["expected_nonzero"] = True
            write_json(fixture.proof, proof)

            result = run_verify(fixture.proof, fixture.root, "v0.0.0")

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("expected-nonzero exit mismatch", result.stderr)

    def test_normal_smoke_rejects_unexpected_nonzero_exit(self) -> None:
        with proof_fixture() as fixture:
            proof = read_json(fixture.proof)
            proof["command"]["observed_exit_status"] = 42
            write_json(fixture.proof, proof)

            result = run_verify(fixture.proof, fixture.root, "v0.0.0")

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unexpected nonzero exit", result.stderr)

    def test_normal_smoke_command_must_match_docs_snippet(self) -> None:
        with proof_fixture() as fixture:
            proof = read_json(fixture.proof)
            proof["command"]["display"] = "m80 run -- echo drifted"
            write_json(fixture.proof, proof)

            result = run_verify(fixture.proof, fixture.root, "v0.0.0")

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("normal command display must match", result.stderr)

    def test_normal_smoke_argv_must_match_docs_snippet(self) -> None:
        with proof_fixture() as fixture:
            proof = read_json(fixture.proof)
            proof["command"]["argv"] = ["m80", "run", "--", "echo", "drifted"]
            write_json(fixture.proof, proof)

            result = run_verify(fixture.proof, fixture.root, "v0.0.0")

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("normal command argv must match", result.stderr)

    def test_stream_swap_fails(self) -> None:
        with proof_fixture() as fixture:
            proof = read_json(fixture.proof)
            proof["stdout"]["excerpt"] = "stderr-marker\n"
            proof["stderr"] = {"excerpt": "stdout-marker\n"}
            proof["stream_expectations"] = {
                "stdout_contains": "stdout-marker",
                "stderr_contains": "stderr-marker",
            }
            write_json(fixture.proof, proof)

            result = run_verify(fixture.proof, fixture.root, "v0.0.0")

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("stdout missing expected capture", result.stderr)

    def test_missing_stdout_capture_fails(self) -> None:
        with proof_fixture() as fixture:
            proof = read_json(fixture.proof)
            proof["stdout"]["excerpt"] = ""
            write_json(fixture.proof, proof)

            result = run_verify(fixture.proof, fixture.root, "v0.0.0")

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("stdout missing expected capture", result.stderr)

    def test_missing_manifest_reference_fails(self) -> None:
        with proof_fixture() as fixture:
            proof = read_json(fixture.proof)
            del proof["host_binaries"]["manifest_path"]
            write_json(fixture.proof, proof)

            result = run_verify(fixture.proof, fixture.root, "v0.0.0")

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("missing field(s): manifest_path", result.stderr)

    def test_hostless_writer_emits_valid_proof(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            metadata = root / "m80-linux-x86_64.bundle.json"
            write_bundle_metadata(metadata)
            proof = root / "m80-quickstart-proof-hostless.json"

            write = subprocess.run(
                [
                    "python3",
                    str(WRITER),
                    "--release-tag",
                    "v0.0.0",
                    "--artifact-root",
                    str(root),
                    "--bundle-metadata",
                    str(metadata),
                    "--out",
                    str(proof),
                ],
                cwd=REPO_ROOT,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            self.assertEqual(write.returncode, 0, write.stderr)

            result = run_verify(proof, root, "v0.0.0")

            self.assertEqual(result.returncode, 0, result.stderr)
            payload = read_json(proof)
            self.assertEqual(payload["proof_kind"], "hostless")
            self.assertEqual(payload["substrate"]["kind"], "hostless")
            self.assertIn("not a real-KVM", payload["substrate"]["summary"])

    def test_release_runbook_and_behavior_doc_name_validator_and_upload_path(self) -> None:
        runbook = (REPO_ROOT / "docs" / "runbook" / "release.md").read_text()
        behavior = (
            REPO_ROOT / "docs" / "behaviors" / "release" / "quickstart-proof-artifacts.md"
        ).read_text()

        for required in [
            "m80-quickstart-proof-hostless.json",
            "scripts/verify-quickstart-proof.py",
            "actions/upload-artifact",
            "hostless",
            "real-kvm",
        ]:
            self.assertIn(required, runbook + behavior)


class proof_fixture:
    def __init__(self, *, proof_kind: str = "hostless") -> None:
        self.proof_kind = proof_kind

    def __enter__(self) -> "proof_fixture":
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.proof = self.root / "proof.json"
        write_bundle_metadata(self.root / "bundle.json")
        write_json(self.root / "host-binaries.manifest.json", {"schema_version": 1})
        (self.root / "stderr.txt").write_text("")
        write_json(self.proof, valid_proof(self.proof_kind))
        return self

    def __exit__(self, *_exc: object) -> None:
        self.tmp.cleanup()


def run_verify(
    proof: Path,
    root: Path,
    release_tag: str,
    *,
    result_out: Path | None = None,
) -> subprocess.CompletedProcess[str]:
    args = [
        "python3",
        str(VERIFY),
        str(proof),
        "--artifact-root",
        str(root),
        "--release-tag",
        release_tag,
    ]
    if result_out is not None:
        args.extend(["--result-out", str(result_out)])
    return subprocess.run(
        args,
        cwd=REPO_ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )


def valid_proof(proof_kind: str) -> dict:
    return {
        "schema_version": 1,
        "proof_kind": proof_kind,
        "release": {
            "requested": "v0.0.0",
            "resolved_tag": "v0.0.0",
            "install_url": "https://github.com/moradology/m80/releases/download/v0.0.0/install.sh",
        },
        "command": {
            "display": "sudo m80 run -- echo hello",
            "argv": ["sudo", "m80", "run", "--", "echo", "hello"],
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
            "root": "/tmp/m80-install-root",
            "active_pointer": "/tmp/m80-install-root/active",
            "default_profile": "/tmp/m80-install-root/profiles/default.toml",
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
            "summary": f"{proof_kind} proof summary",
        },
    }


def write_bundle_metadata(path: Path) -> None:
    write_json(
        path,
        {
            "release_tag": "v0.0.0",
            "m80_version": "v0.0.0",
            "guest_protocol_version": 1,
            "manifest_schema_version": 5,
        },
    )


def read_json(path: Path) -> dict:
    with path.open() as f:
        return json.load(f)


def write_json(path: Path, payload: dict) -> None:
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    unittest.main()
