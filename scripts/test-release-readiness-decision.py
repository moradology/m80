#!/usr/bin/env python3
"""Tests for scripts/release_readiness_decision.py."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "release_readiness_decision.py"
CONFIG = REPO_ROOT / "docs" / "behaviors" / "release" / "release-readiness-lanes.json"
RELEASE_TAG = "v1.2.3"
COMMIT_SHA = "a" * 40
WORKFLOW_RUN_ID = "12345"


class ReleaseReadinessDecisionTests(unittest.TestCase):
    def test_all_green_receipts_emit_decision_and_summary(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            receipts = write_all_receipts(root)
            out = root / "decision.json"

            result = run_decision(receipts, out)

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("release readiness passed", result.stdout)
            decision = json.loads(out.read_text())
            self.assertEqual(decision["status"], "passed")
            self.assertEqual(decision["release_tag"], RELEASE_TAG)
            self.assertEqual(decision["commit_sha"], COMMIT_SHA)
            self.assertIn("real-kvm-quickstart", decision["required_lane_ids"])
            self.assertEqual(decision["blocking_remediations"], [])

    def test_missing_required_lane_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            receipts = write_all_receipts(root)
            del receipts["real-kvm-quickstart"]

            result = run_decision(receipts, root / "decision.json")

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("missing required readiness lane(s): real-kvm-quickstart", result.stderr)

    def test_stale_commit_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            receipts = write_all_receipts(root)
            payload = json.loads(receipts["workflow-policy"].read_text())
            payload["commit_sha"] = "b" * 40
            receipts["workflow-policy"].write_text(json.dumps(payload) + "\n")

            result = run_decision(receipts, root / "decision.json")

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("workflow-policy: stale commit sha", result.stderr)

    def test_fixture_cannot_satisfy_real_kvm(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            receipts = write_all_receipts(root)
            payload = json.loads(receipts["real-kvm-quickstart"].read_text())
            payload["substrate"]["fixture"] = True
            receipts["real-kvm-quickstart"].write_text(json.dumps(payload) + "\n")

            result = run_decision(receipts, root / "decision.json")

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("real-kvm-quickstart: fixture receipt cannot satisfy required substrate", result.stderr)

    def test_failed_public_access_proof_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            receipts = write_all_receipts(root)
            payload = json.loads(receipts["public-access-latest"].read_text())
            payload["auth"]["GH_TOKEN"] = True
            receipts["public-access-latest"].write_text(json.dumps(payload) + "\n")

            result = run_decision(receipts, root / "decision.json")

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("public-access proof used auth: GH_TOKEN", result.stderr)

    def test_public_access_gh_auth_presence_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            receipts = write_all_receipts(root)
            payload = json.loads(receipts["public-access-latest"].read_text())
            payload["auth"]["gh_auth_present"] = True
            receipts["public-access-latest"].write_text(json.dumps(payload) + "\n")

            result = run_decision(receipts, root / "decision.json")

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("public-access proof used auth: gh_auth_present", result.stderr)

    def test_warning_lane_is_accepted_by_policy(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            receipts = write_all_receipts(root, include_warning=True)

            result = run_decision(receipts, root / "decision.json")

            self.assertEqual(result.returncode, 0, result.stderr)
            decision = json.loads((root / "decision.json").read_text())
            freshness = next(row for row in decision["lane_status"] if row["lane_id"] == "latest-freshness")
            self.assertEqual(freshness["status"], "warning")
            self.assertEqual(freshness["publish_effect"], "warn")

    def test_unknown_lane_state_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            receipts = write_all_receipts(root)
            payload = json.loads(receipts["hostless-quickstart"].read_text())
            payload["status"] = "green"
            receipts["hostless-quickstart"].write_text(json.dumps(payload) + "\n")

            result = run_decision(receipts, root / "decision.json")

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("hostless-quickstart: unknown lane state: green", result.stderr)


def run_decision(receipts: dict[str, Path], out: Path) -> subprocess.CompletedProcess[str]:
    args = [
        "python3",
        str(SCRIPT),
        "--config",
        str(CONFIG),
        "--release-tag",
        RELEASE_TAG,
        "--commit-sha",
        COMMIT_SHA,
        "--workflow-run-id",
        WORKFLOW_RUN_ID,
        "--out",
        str(out),
        "--write",
    ]
    for lane_id, path in sorted(receipts.items()):
        args.extend(["--receipt", f"{lane_id}={path}"])
    return subprocess.run(args, cwd=REPO_ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def write_all_receipts(root: Path, *, include_warning: bool = False) -> dict[str, Path]:
    receipts = {
        "workflow-policy": write_lane_receipt(root, "workflow-policy", "workflow-policy", "workflow-policy-report", "github-actions", "workflow_policy_sha256"),
        "release-bundle-integrity": write_lane_receipt(root, "release-bundle-integrity", "release-integrity", "release-integrity-predicate", "github-actions", "predicate_sha256"),
        "docs-command": write_docs_receipt(root),
        "hostless-quickstart": write_lane_receipt(root, "hostless-quickstart", "hostless-quickstart", "quickstart-proof", "hostless", "proof_sha256", fixture=True),
        "real-kvm-quickstart": write_lane_receipt(root, "real-kvm-quickstart", "real-kvm-smoke", "quickstart-proof", "real-kvm", "proof_sha256"),
        "public-access-latest": write_public_access_receipt(root),
    }
    if include_warning:
        receipts["latest-freshness"] = write_lane_receipt(
            root,
            "latest-freshness",
            "freshness",
            "freshness-proof",
            "public-github",
            "proof_sha256",
            status="warning",
        )
    return receipts


def write_lane_receipt(
    root: Path,
    lane_id: str,
    lane_kind: str,
    proof_kind: str,
    substrate_kind: str,
    digest_field: str,
    *,
    status: str = "passed",
    fixture: bool = False,
) -> Path:
    digest = "sha256:" + hashlib_for_lane(lane_id)
    payload = {
        "schema_version": 1,
        "kind": "m80_release_readiness_lane_receipt",
        "lane_id": lane_id,
        "lane_kind": lane_kind,
        "proof_kind": proof_kind,
        "status": status,
        "release_tag": RELEASE_TAG,
        "commit_sha": COMMIT_SHA,
        "workflow_run_id": WORKFLOW_RUN_ID,
        "verification_time": "2026-05-23T00:00:00Z",
        "substrate": {
            "kind": substrate_kind,
            "fixture": fixture,
        },
        "artifact": {
            "path": f"{lane_id}.json",
            "sha256": digest,
            "size_bytes": 2,
        },
        digest_field: digest,
        "remediation": {
            "command": f"br show m80-o3uh9",
            "bead_id": None,
        },
    }
    path = root / f"{lane_id}.json"
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    return path


def write_docs_receipt(root: Path) -> Path:
    digest = "sha256:" + hashlib_for_lane("docs-command")
    payload = {
        "schema_version": 1,
        "kind": "m80_release_readiness_docs_command",
        "lane_id": "docs-command",
        "lane_kind": "docs-command",
        "proof_kind": "docs-command-receipt",
        "status": "passed",
        "release_tag": RELEASE_TAG,
        "commit_sha": COMMIT_SHA,
        "workflow_run_id": WORKFLOW_RUN_ID,
        "verification_time": "2026-05-23T00:00:00Z",
        "substrate": {
            "kind": "github-actions",
            "fixture": False,
        },
        "repository": "moradology/m80",
        "snippet_sources": [{"path": "README.md"}],
        "rendered_install_commands": ["curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh"],
        "command_digest_sha256": digest,
        "remediation": {
            "command": "python3 scripts/release_docs_command_receipt.py",
            "bead_id": None,
        },
    }
    path = root / "docs-command.json"
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    return path


def write_public_access_receipt(root: Path) -> Path:
    digest = "sha256:" + hashlib_for_lane("public-access-latest")
    payload = {
        "schema_version": 1,
        "kind": "m80_release_readiness_public_access",
        "lane_id": "public-access-latest",
        "proof_kind": "public-access-proof",
        "status": "passed",
        "release_tag": RELEASE_TAG,
        "commit_sha": COMMIT_SHA,
        "workflow_run_id": None,
        "substrate": {
            "kind": "public-github",
            "fixture": False,
        },
        "auth": {
            "GH_TOKEN": False,
            "GITHUB_TOKEN": False,
            "gh_auth_present": False,
            "authorization_header_used": False,
        },
        "asset_manifest": {
            "name": "m80-release-assets.json",
            "sha256": digest,
            "size_bytes": 2,
        },
        "proof_sha256": digest,
        "remediation": {
            "command": "br show m80-o3uh9.21.7",
            "bead_id": None,
        },
    }
    path = root / "public-access-latest.json"
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    return path


def hashlib_for_lane(lane_id: str) -> str:
    return (lane_id.encode().hex() * 64)[:64].ljust(64, "0")


if __name__ == "__main__":
    unittest.main(verbosity=2)
