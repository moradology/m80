#!/usr/bin/env python3
"""Tests for scripts/verify-release-readiness-config.py."""

from __future__ import annotations

import copy
import json
from pathlib import Path
import subprocess
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
VERIFY = REPO_ROOT / "scripts" / "verify-release-readiness-config.py"
CONFIG = REPO_ROOT / "docs" / "behaviors" / "release" / "release-readiness-lanes.json"


class ReleaseReadinessConfigTest(unittest.TestCase):
    def test_repo_config_passes(self) -> None:
        result = run_verify(CONFIG)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("release readiness config ok", result.stdout)

    def test_missing_required_lane_fails(self) -> None:
        config = valid_config()
        config["lanes"] = [lane for lane in config["lanes"] if lane["id"] != "real-kvm-quickstart"]

        result = run_verify(write_config(config))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing required readiness lane: real-kvm-quickstart", result.stderr)
        self.assertIn("unknown required lane id: real-kvm-quickstart", result.stderr)

    def test_missing_required_stage_fails(self) -> None:
        config = valid_config()
        config["readiness_stages"] = [
            stage for stage in config["readiness_stages"] if stage["id"] != "pre-latest"
        ]

        result = run_verify(write_config(config))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing required readiness stage: pre-latest", result.stderr)

    def test_stage_unknown_lane_fails(self) -> None:
        config = valid_config()
        config["readiness_stages"][0]["required_lane_ids"].append("ghost-lane")

        result = run_verify(write_config(config))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unknown required lane id: ghost-lane", result.stderr)

    def test_pre_upload_stage_shape_is_fixed(self) -> None:
        config = valid_config()
        stage = stage_by_id(config, "pre-upload")
        stage["required_lane_ids"].append("real-kvm-quickstart")

        result = run_verify(write_config(config))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("required_lane_ids must be", result.stderr)

    def test_duplicate_lane_id_fails(self) -> None:
        config = valid_config()
        config["lanes"].append(copy.deepcopy(config["lanes"][0]))

        result = run_verify(write_config(config))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("duplicate lane id", result.stderr)

    def test_unknown_lane_kind_fails(self) -> None:
        config = valid_config()
        config["lanes"][0]["lane_kind"] = "maybe-release"

        result = run_verify(write_config(config))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unknown lane_kind: maybe-release", result.stderr)

    def test_missing_required_status_fails(self) -> None:
        config = valid_config()
        config["status_taxonomy"] = [
            status for status in config["status_taxonomy"] if status["id"] != "stale"
        ]

        result = run_verify(write_config(config))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing required status: stale", result.stderr)

    def test_fixture_lane_cannot_satisfy_real_kvm(self) -> None:
        config = valid_config()
        lane = lane_by_id(config, "real-kvm-quickstart")
        lane["allowed_substrates"] = ["local-fixture"]

        result = run_verify(write_config(config))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("lane_kind real-kvm-smoke requires substrate(s) ['real-kvm']", result.stderr)
        self.assertIn("publish-blocking lanes cannot be satisfied by local-fixture", result.stderr)

    def test_missing_required_digest_field_fails(self) -> None:
        config = valid_config()
        lane_by_id(config, "release-bundle-integrity")["digest_field"] = ""

        result = run_verify(write_config(config))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("digest_field must be a nonempty field path", result.stderr)
        self.assertIn("publish-blocking lanes must name digest_field", result.stderr)

    def test_warning_publish_blocking_lane_requires_policy_text(self) -> None:
        config = valid_config()
        lane = lane_by_id(config, "latest-freshness")
        lane["publish_blocking"] = True
        lane["policy_text"] = ""

        result = run_verify(write_config(config))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("warning publish-blocking lane requires policy_text", result.stderr)

    def test_malformed_remediation_command_fails(self) -> None:
        config = valid_config()
        lane_by_id(config, "workflow-policy")["remediation"]["command"] = "rm -rf /"

        result = run_verify(write_config(config))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("malformed remediation command starts with unsupported tool: rm", result.stderr)

    def test_shell_chaining_in_remediation_command_fails(self) -> None:
        config = valid_config()
        lane_by_id(config, "workflow-policy")["remediation"]["command"] = (
            "python3 scripts/lint-github-workflows.py && rm -rf /"
        )

        result = run_verify(write_config(config))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("malformed remediation command contains '&&'", result.stderr)


def run_verify(config: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["python3", str(VERIFY), "--config", str(config)],
        cwd=REPO_ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def write_config(config: dict) -> Path:
    root = tempfile.TemporaryDirectory()
    path = Path(root.name) / "readiness.json"
    path.write_text(json.dumps(config))
    CLEANUPS.append(root)
    return path


def valid_config() -> dict:
    return json.loads(CONFIG.read_text())


def lane_by_id(config: dict, lane_id: str) -> dict:
    for lane in config["lanes"]:
        if lane["id"] == lane_id:
            return lane
    raise AssertionError(f"lane missing from fixture: {lane_id}")


def stage_by_id(config: dict, stage_id: str) -> dict:
    for stage in config["readiness_stages"]:
        if stage["id"] == stage_id:
            return stage
    raise AssertionError(f"stage missing from fixture: {stage_id}")


CLEANUPS: list[tempfile.TemporaryDirectory] = []


if __name__ == "__main__":
    try:
        unittest.main()
    finally:
        for cleanup in CLEANUPS:
            cleanup.cleanup()
