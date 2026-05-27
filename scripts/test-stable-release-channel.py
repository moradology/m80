#!/usr/bin/env python3
"""Tests for stable release-channel metadata validation."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import tempfile
import unittest

from release_url_contract import release_asset_url
from stable_release_channel import (
    BUNDLE_NAME,
    INTEGRITY_ATTESTATION_BUNDLE_NAME,
    METADATA_NAME,
    REQUIRED_PUBLIC_ASSETS,
    validate_stable_release_metadata,
)


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "stable_release_channel.py"


class StableReleaseChannelTest(unittest.TestCase):
    def test_accepts_stable_release_metadata_and_asset_index(self) -> None:
        eligibility = validate_stable_release_metadata(base_release_metadata(), asset_index=base_asset_index())

        self.assertEqual(eligibility.tag, "v1.2.3")
        self.assertIn("install.sh", eligibility.required_assets)

    def test_cli_renders_accepted_stable_release_json(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            metadata = write_json(root / "release.json", base_release_metadata())
            index = write_json(root / "assets.json", base_asset_index())

            result = subprocess.run(
                [
                    "python3",
                    str(SCRIPT),
                    "--release-metadata",
                    str(metadata),
                    "--asset-index",
                    str(index),
                    "--expected-tag",
                    "v1.2.3",
                    "--json",
                ],
                cwd=REPO_ROOT,
                check=True,
                text=True,
                capture_output=True,
            )

        payload = json.loads(result.stdout)
        self.assertTrue(payload["stable_release_eligible"])
        self.assertEqual(payload["tag"], "v1.2.3")

    def test_rejects_draft_release_before_install_asset_download(self) -> None:
        release = base_release_metadata(draft=True)

        with self.assertRaisesRegex(ValueError, "draft release"):
            validate_stable_release_metadata(release, asset_index=base_asset_index())

    def test_rejects_prerelease_before_install_asset_download(self) -> None:
        release = base_release_metadata(prerelease=True)

        with self.assertRaisesRegex(ValueError, "prerelease"):
            validate_stable_release_metadata(release, asset_index=base_asset_index())

    def test_rejects_prerelease_tag_suffix(self) -> None:
        release = base_release_metadata(tag="v1.2.3-rc.1")

        with self.assertRaisesRegex(ValueError, "no prerelease suffix"):
            validate_stable_release_metadata(release, asset_index=base_asset_index(tag="v1.2.3-rc.1"))

    def test_rejects_missing_install_asset(self) -> None:
        release = base_release_metadata(omit_assets={"install.sh"})

        with self.assertRaisesRegex(ValueError, "missing required public asset.*install.sh"):
            validate_stable_release_metadata(release, asset_index=base_asset_index())

    def test_rejects_wrong_public_asset_url(self) -> None:
        release = base_release_metadata()
        for asset in release["assets"]:
            if asset["name"] == "install.sh":
                asset["browser_download_url"] = "https://github.com/other/m80/releases/download/v1.2.3/install.sh"

        with self.assertRaisesRegex(ValueError, "asset URL mismatch for install.sh"):
            validate_stable_release_metadata(release, asset_index=base_asset_index())

    def test_rejects_asset_index_tag_mismatch(self) -> None:
        with self.assertRaisesRegex(ValueError, "release asset index tag mismatch"):
            validate_stable_release_metadata(base_release_metadata(), asset_index=base_asset_index(tag="v9.9.9"))

    def test_rejects_asset_index_m80_version_mismatch(self) -> None:
        index = base_asset_index()
        index["assets"][0]["m80_version"] = "v9.9.9"

        with self.assertRaisesRegex(ValueError, "m80_version mismatch"):
            validate_stable_release_metadata(base_release_metadata(), asset_index=index)


def base_release_metadata(
    *,
    tag: str = "v1.2.3",
    draft: bool = False,
    prerelease: bool = False,
    omit_assets: set[str] | None = None,
) -> dict:
    omitted = omit_assets or set()
    return {
        "tag_name": tag,
        "draft": draft,
        "prerelease": prerelease,
        "assets": [
            {
                "name": name,
                "browser_download_url": release_asset_url(tag, name),
            }
            for name in REQUIRED_PUBLIC_ASSETS
            if name not in omitted
        ],
    }


def base_asset_index(*, tag: str = "v1.2.3") -> dict:
    return {
        "schema_version": 2,
        "release_tag": tag,
        "assets": [
            {
                "name": BUNDLE_NAME,
                "url": release_asset_url(tag, BUNDLE_NAME),
                "sha256": "a" * 64,
                "size_bytes": 42,
                "metadata_name": METADATA_NAME,
                "metadata_sha256": "b" * 64,
                "signature_name": None,
                "attestation_name": INTEGRITY_ATTESTATION_BUNDLE_NAME,
                "target": "linux-x86_64",
                "os": "linux",
                "arch": "x86_64",
                "image_kind": "minimal",
                "release_tag": tag,
                "m80_version": tag,
                "guest_protocol_version": 1,
                "manifest_schema_version": 1,
                "expected_firecracker_version": "v1.15.1",
            }
        ],
    }


def write_json(path: Path, value: dict) -> Path:
    path.write_text(json.dumps(value))
    return path


if __name__ == "__main__":
    unittest.main()
