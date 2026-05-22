#!/usr/bin/env python3
"""Tests for scripts/release_publication_plan.py."""

from __future__ import annotations

import json
from contextlib import contextmanager
from pathlib import Path
import subprocess
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "release_publication_plan.py"
RELEASE_TAG = "v0.2.7"


class ReleasePublicationPlanTest(unittest.TestCase):
    def test_absent_release_creates_draft_upload_publish_plan(self) -> None:
        with fixture() as root:
            write_json(root / "github-release.json", {})

            result = run_plan(root)

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), "create_draft_upload_publish")
            plan = json.loads((root / "plan.json").read_text())
            self.assertEqual(plan["action"], "create_draft_upload_publish")
            self.assertEqual(plan["release_state"], "absent")
            self.assertEqual(plan["expected_public_asset_count"], 2)
            self.assertEqual(plan["observed_remote_asset_count"], 0)
            self.assertIsNone(plan["manual_recovery"])

    def test_existing_public_release_with_complete_asset_metadata_is_validate_only(self) -> None:
        with fixture() as root:
            write_release_metadata(root / "github-release.json")

            result = run_plan(root)

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), "validate_existing_public_release")
            plan = json.loads((root / "plan.json").read_text())
            self.assertEqual(plan["action"], "validate_existing_public_release")
            self.assertEqual(plan["release_state"], "public")
            self.assertEqual(plan["observed_remote_asset_count"], 2)

    def test_existing_draft_release_requires_manual_delete(self) -> None:
        with fixture() as root:
            write_release_metadata(root / "github-release.json", draft=True, assets=["install.sh"])

            result = run_plan(root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("draft release exists", result.stderr)
            plan = json.loads((root / "plan.json").read_text())
            self.assertEqual(plan["action"], "fail_manual_recovery_required")
            self.assertEqual(plan["release_state"], "draft")
            self.assertEqual(plan["missing_assets"], ["SHA256SUMS"])
            self.assertEqual(plan["manual_recovery"], f"gh release delete {RELEASE_TAG} --yes")

    def test_existing_public_release_missing_asset_fails_without_clobber(self) -> None:
        with fixture() as root:
            write_release_metadata(root / "github-release.json", assets=["install.sh"])

            result = run_plan(root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("asset metadata mismatch", result.stderr)
            plan = json.loads((root / "plan.json").read_text())
            self.assertEqual(plan["action"], "fail_manual_recovery_required")
            self.assertEqual(plan["missing_assets"], ["SHA256SUMS"])
            self.assertIn("will not clobber public assets", plan["manual_recovery"])

    def test_existing_public_release_size_mismatch_fails_before_upload(self) -> None:
        with fixture() as root:
            write_release_metadata(root / "github-release.json", size_updates={"install.sh": 99})

            result = run_plan(root)

            self.assertNotEqual(result.returncode, 0)
            plan = json.loads((root / "plan.json").read_text())
            self.assertEqual(
                plan["size_mismatches"],
                [{"expected_size_bytes": 7, "name": "install.sh", "observed_size_bytes": 99}],
            )


def run_plan(tmp: str | Path) -> subprocess.CompletedProcess[str]:
    root = Path(tmp)
    return subprocess.run(
        [
            "python3",
            str(SCRIPT),
            "--dist-dir",
            str(root),
            "--release-tag",
            RELEASE_TAG,
            "--release-metadata",
            str(root / "github-release.json"),
            "--out",
            str(root / "plan.json"),
            "--generated-at",
            "2026-05-22T00:00:00Z",
            "--write",
        ],
        text=True,
        capture_output=True,
        check=False,
    )


def write_release_metadata(
    path: Path,
    *,
    draft: bool = False,
    prerelease: bool = False,
    assets: list[str] | None = None,
    size_updates: dict[str, int] | None = None,
) -> None:
    names = assets or ["install.sh", "SHA256SUMS"]
    sizes = {"install.sh": 7, "SHA256SUMS": 11}
    sizes.update(size_updates or {})
    write_json(
        path,
        {
            "tagName": RELEASE_TAG,
            "url": f"https://github.com/moradology/m80/releases/tag/{RELEASE_TAG}",
            "isDraft": draft,
            "isPrerelease": prerelease,
            "assets": [{"name": name, "size": sizes[name]} for name in names],
        },
    )


def write_json(path: Path, payload: dict) -> Path:
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    return path


def write_manifest(root: Path) -> None:
    write_json(
        root / "m80-release-upload-manifest.json",
        {
            "schema_version": 1,
            "release_tag": RELEASE_TAG,
            "public_assets": [
                {
                    "name": "install.sh",
                    "kind": "installer",
                    "sha256": "0" * 64,
                    "size_bytes": 7,
                    "integrity_subject": True,
                },
                {
                    "name": "SHA256SUMS",
                    "kind": "checksum-manifest",
                    "sha256": "1" * 64,
                    "size_bytes": 11,
                    "integrity_subject": True,
                },
            ],
            "non_public_workflow_artifacts": [],
        },
    )


@contextmanager
def fixture():
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        write_manifest(root)
        yield root


if __name__ == "__main__":
    unittest.main()
