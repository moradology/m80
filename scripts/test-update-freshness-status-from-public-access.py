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
from stable_release_channel import REQUIRED_PUBLIC_ASSETS


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
            self.assertEqual(len(status["public_assets"]), len(REQUIRED_PUBLIC_ASSETS))
            self.assertEqual(
                status["safety_floor"],
                {
                    "schema_version": 1,
                    "published_at": status["generated_at"],
                    "minimum_safe_tag": None,
                    "yanked_releases": [],
                },
            )

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

    def test_safety_floor_input_is_embedded_and_validated(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            artifact_root = root / "docs" / "behaviors" / "release"
            artifact_root.mkdir(parents=True)
            receipt_path = artifact_root / "release-readiness-public-access.json"
            status_path = artifact_root / "freshness-status-docs.json"
            floor_path = artifact_root / "safety-floor.json"
            receipt = valid_receipt()
            floor = safety_floor(
                receipt,
                minimum_safe_tag=minimum_safe_tag("v0.2.6", "v0.2.7"),
                yanked_releases=[yanked_release(receipt, "v0.2.5", replacement_tag="v0.2.7")],
            )
            receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
            floor_path.write_text(json.dumps(floor, indent=2, sort_keys=True) + "\n")

            result = run_update(receipt_path, status_path, "--safety-floor", str(floor_path))

            self.assertEqual(result.returncode, 0, result.stderr)
            status = json.loads(status_path.read_text())
            self.assertEqual(status["safety_floor"], floor)

    def test_yanked_latest_with_replacement_is_allowed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            artifact_root = root / "docs" / "behaviors" / "release"
            artifact_root.mkdir(parents=True)
            receipt_path = artifact_root / "release-readiness-public-access.json"
            status_path = artifact_root / "freshness-status-docs.json"
            floor_path = artifact_root / "safety-floor.json"
            receipt = valid_receipt()
            floor = safety_floor(
                receipt,
                minimum_safe_tag=None,
                yanked_releases=[
                    yanked_release(
                        receipt,
                        receipt["resolved_latest_tag"],
                        replacement_tag="v0.2.6",
                    )
                ],
            )
            receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
            floor_path.write_text(json.dumps(floor, indent=2, sort_keys=True) + "\n")

            result = run_update(receipt_path, status_path, "--safety-floor", str(floor_path))

            self.assertEqual(result.returncode, 0, result.stderr)
            status = json.loads(status_path.read_text())
            self.assertEqual(
                status["safety_floor"]["yanked_releases"][0]["replacement_command"],
                pinned_install_command("v0.2.6"),
            )

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

    def test_validator_error_does_not_publish_green_status(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            artifact_root = root / "docs" / "behaviors" / "release"
            artifact_root.mkdir(parents=True)
            receipt_path = artifact_root / "release-readiness-public-access.json"
            status_path = artifact_root / "freshness-status-docs.json"
            status_path.write_text('{"status":"previous"}\n')
            receipt_path.write_text(json.dumps(valid_receipt(), indent=2, sort_keys=True) + "\n")

            result = run_update(
                receipt_path,
                status_path,
                "--expected-stable-tag",
                "v9.0.0",
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("public_green requires resolved_latest_tag == expected_highest_stable_tag", result.stderr)
            self.assertEqual(status_path.read_text(), '{"status":"previous"}\n')

    def test_invalid_safety_floor_does_not_publish_green_status(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            artifact_root = root / "docs" / "behaviors" / "release"
            artifact_root.mkdir(parents=True)
            receipt_path = artifact_root / "release-readiness-public-access.json"
            status_path = artifact_root / "freshness-status-docs.json"
            floor_path = artifact_root / "safety-floor.json"
            status_path.write_text('{"status":"previous"}\n')
            receipt = valid_receipt()
            floor = safety_floor(
                receipt,
                minimum_safe_tag=minimum_safe_tag("v9.0.0", "v9.0.0"),
                yanked_releases=[],
            )
            receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
            floor_path.write_text(json.dumps(floor, indent=2, sort_keys=True) + "\n")

            result = run_update(receipt_path, status_path, "--safety-floor", str(floor_path))

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("minimum_safe_tag tag v9.0.0", result.stderr)
            self.assertEqual(status_path.read_text(), '{"status":"previous"}\n')

    def test_stale_safety_advisory_does_not_publish_green_status(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            artifact_root = root / "docs" / "behaviors" / "release"
            artifact_root.mkdir(parents=True)
            receipt_path = artifact_root / "release-readiness-public-access.json"
            status_path = artifact_root / "freshness-status-docs.json"
            floor_path = artifact_root / "safety-floor.json"
            status_path.write_text('{"status":"previous"}\n')
            receipt = valid_receipt()
            floor = safety_floor(
                receipt,
                minimum_safe_tag=None,
                yanked_releases=[yanked_release(receipt, "v0.2.5", replacement_tag="v0.2.7")],
            )
            floor["yanked_releases"][0]["published_at"] = "2099-01-01T00:00:00Z"
            receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
            floor_path.write_text(json.dumps(floor, indent=2, sort_keys=True) + "\n")

            result = run_update(receipt_path, status_path, "--safety-floor", str(floor_path))

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("yanked_releases[0] published_at for v0.2.5", result.stderr)
            self.assertEqual(status_path.read_text(), '{"status":"previous"}\n')


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


def safety_floor(
    receipt: dict,
    *,
    minimum_safe_tag: dict | None,
    yanked_releases: list[dict],
) -> dict:
    return {
        "schema_version": 1,
        "published_at": receipt["verification_time"],
        "minimum_safe_tag": minimum_safe_tag,
        "yanked_releases": yanked_releases,
    }


def minimum_safe_tag(tag: str, replacement_tag: str) -> dict:
    return {
        "tag": tag,
        "reason": "security floor",
        "advisory_url": None,
        "issue_id": "m80-o3uh9.21.9.4",
        "replacement_command": pinned_install_command(replacement_tag),
    }


def yanked_release(receipt: dict, tag: str, *, replacement_tag: str) -> dict:
    return {
        "tag": tag,
        "reason": "yanked release",
        "advisory_url": "https://github.com/moradology/m80/issues/1",
        "issue_id": None,
        "published_at": receipt["verification_time"],
        "replacement_command": pinned_install_command(replacement_tag),
        "no_replacement_reason": None,
    }


def pinned_install_command(tag: str) -> str:
    return f"curl -fsSL https://github.com/moradology/m80/releases/download/{tag}/install.sh | sudo sh"


if __name__ == "__main__":
    unittest.main()
