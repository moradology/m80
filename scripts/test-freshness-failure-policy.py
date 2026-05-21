#!/usr/bin/env python3
"""Tests for scripts/verify-freshness-failure-policy.py."""

from __future__ import annotations

import copy
import json
from pathlib import Path
import subprocess
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
VERIFY = REPO_ROOT / "scripts" / "verify-freshness-failure-policy.py"
CONFIG = REPO_ROOT / "docs" / "behaviors" / "release" / "freshness-failure-policy.json"


class FreshnessFailurePolicyTest(unittest.TestCase):
    def test_repo_config_passes(self) -> None:
        result = run_verify(CONFIG)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("freshness failure policy ok", result.stdout)

    def test_unknown_class_fails(self) -> None:
        config = valid_config()
        extra = copy.deepcopy(config["failure_classes"][0])
        extra["id"] = "surprise-drift"
        config["failure_classes"].append(extra)

        result = run_verify(write_config(config))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unknown failure class", result.stderr)

    def test_duplicate_class_fails(self) -> None:
        config = valid_config()
        config["failure_classes"].append(copy.deepcopy(config["failure_classes"][0]))

        result = run_verify(write_config(config))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("duplicate failure class", result.stderr)

    def test_missing_disposition_fails(self) -> None:
        config = valid_config()
        del class_by_id(config, "docs-drift")["disposition"]

        result = run_verify(write_config(config))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing required field disposition", result.stderr)

    def test_invalid_disposition_fails(self) -> None:
        config = valid_config()
        class_by_id(config, "docs-drift")["disposition"] = "shrug"

        result = run_verify(write_config(config))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("invalid disposition: shrug", result.stderr)

    def test_verifier_emitted_class_absent_from_config_fails(self) -> None:
        config = valid_config()
        config["failure_classes"] = [
            row for row in config["failure_classes"] if row["id"] != "missing-public-asset"
        ]

        result = run_verify(write_config(config))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("verifier-emitted class absent from config: missing-public-asset", result.stderr)

    def test_extra_verifier_emitted_class_absent_from_config_fails(self) -> None:
        result = run_verify(CONFIG, "--emitted-class", "future-failure")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("verifier-emitted class absent from config: future-failure", result.stderr)

    def test_disposition_booleans_must_match_action(self) -> None:
        config = valid_config()
        class_by_id(config, "stale-latest")["blocks_next_release"] = False

        result = run_verify(write_config(config))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("block-next-release-latest disposition requires blocks_next_release=true", result.stderr)

    def test_repair_command_rejects_shell_chaining(self) -> None:
        config = valid_config()
        class_by_id(config, "docs-drift")["repair_command"] = "python3 scripts/release_freshness.py && rm -rf /"

        result = run_verify(write_config(config))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("repair_command contains '&&'", result.stderr)


def run_verify(config: Path, *extra_args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["python3", str(VERIFY), "--config", str(config), *extra_args],
        cwd=REPO_ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def write_config(config: dict) -> Path:
    root = tempfile.TemporaryDirectory()
    path = Path(root.name) / "freshness-failure-policy.json"
    path.write_text(json.dumps(config))
    CLEANUPS.append(root)
    return path


def valid_config() -> dict:
    return json.loads(CONFIG.read_text())


def class_by_id(config: dict, class_id: str) -> dict:
    for row in config["failure_classes"]:
        if row["id"] == class_id:
            return row
    raise AssertionError(f"class missing from fixture: {class_id}")


CLEANUPS: list[tempfile.TemporaryDirectory] = []


if __name__ == "__main__":
    try:
        unittest.main()
    finally:
        for cleanup in CLEANUPS:
            cleanup.cleanup()
