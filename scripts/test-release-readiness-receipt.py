#!/usr/bin/env python3
"""Tests for scripts/release_readiness_receipt.py."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "release_readiness_receipt.py"
CONFIG = REPO_ROOT / "docs" / "behaviors" / "release" / "release-readiness-lanes.json"


class ReleaseReadinessReceiptTests(unittest.TestCase):
    def test_all_local_lanes_write_green_receipts(self) -> None:
        for lane_id, substrate, digest_field in [
            ("workflow-policy", "github-actions", "workflow_policy_sha256"),
            ("release-bundle-integrity", "github-actions", "predicate_sha256"),
            ("hostless-quickstart", "hostless", "proof_sha256"),
        ]:
            with self.subTest(lane_id=lane_id), tempfile.TemporaryDirectory() as root_name:
                root = Path(root_name)
                artifact = root / f"{lane_id}.json"
                artifact.write_text(json.dumps({"lane": lane_id}) + "\n")
                out = root / f"{lane_id}-receipt.json"

                result = run_receipt(
                    root,
                    lane_id=lane_id,
                    substrate=substrate,
                    artifact=artifact,
                    out=out,
                    workflow_run_id="12345",
                )

                self.assertEqual(result.returncode, 0, result.stderr)
                receipt = json.loads(out.read_text())
                self.assertEqual(receipt["kind"], "m80_release_readiness_lane_receipt")
                self.assertEqual(receipt["lane_id"], lane_id)
                self.assertEqual(receipt["status"], "passed")
                self.assertEqual(receipt["release_tag"], "v1.2.3")
                self.assertEqual(receipt["commit_sha"], "a" * 40)
                self.assertEqual(receipt["workflow_run_id"], "12345")
                self.assertEqual(receipt["substrate"]["kind"], substrate)
                self.assertEqual(receipt[digest_field], receipt["artifact"]["sha256"])

    def test_unknown_lane_id_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as root_name:
            root = Path(root_name)
            artifact = root / "proof.json"
            artifact.write_text("{}\n")
            result = run_receipt(root, lane_id="not-a-lane", substrate="hostless", artifact=artifact)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unknown lane id", result.stderr)

    def test_release_integrity_missing_digest_field_fails_verification(self) -> None:
        with tempfile.TemporaryDirectory() as root_name:
            root = Path(root_name)
            artifact = root / "predicate.json"
            artifact.write_text("{}\n")
            out = root / "receipt.json"
            result = run_receipt(
                root,
                lane_id="release-bundle-integrity",
                substrate="github-actions",
                artifact=artifact,
                out=out,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            payload = json.loads(out.read_text())
            del payload["predicate_sha256"]
            out.write_text(json.dumps(payload) + "\n")

            verify = run_receipt(
                root,
                lane_id="release-bundle-integrity",
                substrate="github-actions",
                artifact=artifact,
                out=out,
                write=False,
            )

            self.assertNotEqual(verify.returncode, 0)
            self.assertIn("missing field(s): predicate_sha256", verify.stderr)

    def test_stale_tag_and_commit_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as root_name:
            root = Path(root_name)
            artifact = root / "policy.json"
            artifact.write_text("{}\n")
            out = root / "receipt.json"
            result = run_receipt(root, lane_id="workflow-policy", substrate="github-actions", artifact=artifact, out=out)
            self.assertEqual(result.returncode, 0, result.stderr)

            stale_tag = run_receipt(
                root,
                lane_id="workflow-policy",
                substrate="github-actions",
                artifact=artifact,
                out=out,
                write=False,
                expected_release_tag="v9.9.9",
            )
            stale_commit = run_receipt(
                root,
                lane_id="workflow-policy",
                substrate="github-actions",
                artifact=artifact,
                out=out,
                write=False,
                expected_commit_sha="b" * 40,
            )

            self.assertNotEqual(stale_tag.returncode, 0)
            self.assertIn("stale release tag", stale_tag.stderr)
            self.assertNotEqual(stale_commit.returncode, 0)
            self.assertIn("stale commit sha", stale_commit.stderr)

    def test_unknown_status_and_malformed_remediation_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as root_name:
            root = Path(root_name)
            artifact = root / "policy.json"
            artifact.write_text("{}\n")
            bad_status = run_receipt(
                root,
                lane_id="workflow-policy",
                substrate="github-actions",
                artifact=artifact,
                status="green-ish",
            )
            bad_command = run_receipt(
                root,
                lane_id="workflow-policy",
                substrate="github-actions",
                artifact=artifact,
                remediation_command="python3 scripts/lint-github-workflows.py; rm -rf /",
            )

            self.assertNotEqual(bad_status.returncode, 0)
            self.assertIn("unknown status", bad_status.stderr)
            self.assertNotEqual(bad_command.returncode, 0)
            self.assertIn("malformed remediation command", bad_command.stderr)

    def test_hostless_fixture_cannot_claim_stronger_substrate(self) -> None:
        with tempfile.TemporaryDirectory() as root_name:
            root = Path(root_name)
            artifact = root / "proof.json"
            artifact.write_text("{}\n")
            result = run_receipt(
                root,
                lane_id="hostless-quickstart",
                substrate="github-actions",
                artifact=artifact,
                fixture=True,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("wrong-substrate", result.stderr)


def run_receipt(
    root: Path,
    *,
    lane_id: str,
    substrate: str,
    artifact: Path,
    out: Path | None = None,
    status: str = "passed",
    workflow_run_id: str | None = None,
    remediation_command: str | None = None,
    expected_release_tag: str | None = None,
    expected_commit_sha: str | None = None,
    fixture: bool = False,
    write: bool = True,
) -> subprocess.CompletedProcess[str]:
    args = [
        "python3",
        str(SCRIPT),
        "--config",
        str(CONFIG),
        "--lane-id",
        lane_id,
        "--status",
        status,
        "--release-tag",
        "v1.2.3",
        "--commit-sha",
        "a" * 40,
        "--substrate-kind",
        substrate,
        "--artifact-root",
        str(root),
        "--artifact",
        str(artifact),
        "--out",
        str(out or (root / "receipt.json")),
    ]
    if workflow_run_id is not None:
        args.extend(["--workflow-run-id", workflow_run_id])
    if remediation_command is not None:
        args.extend(["--remediation-command", remediation_command])
    if expected_release_tag is not None:
        args.extend(["--expected-release-tag", expected_release_tag])
    if expected_commit_sha is not None:
        args.extend(["--expected-commit-sha", expected_commit_sha])
    if fixture:
        args.append("--fixture")
    if write:
        args.append("--write")
    return subprocess.run(args, cwd=REPO_ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


if __name__ == "__main__":
    unittest.main(verbosity=2)
