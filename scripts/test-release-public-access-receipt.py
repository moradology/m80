#!/usr/bin/env python3
"""Tests for scripts/release_public_access_receipt.py."""

from __future__ import annotations

import copy
import importlib.util
from pathlib import Path
import sys
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "release_public_access_receipt.py"
spec = importlib.util.spec_from_file_location("release_public_access_receipt", SCRIPT)
public_access = importlib.util.module_from_spec(spec)
assert spec.loader is not None
sys.modules[spec.name] = public_access
spec.loader.exec_module(public_access)


REPOSITORY = "moradology/m80"
TAG = "v0.2.7"
COMMIT = "388d2d5aa13418e22c1144963b0f66dd3588a92b"
INSTALL_SHA = "a" * 64


class PublicAccessReceiptTests(unittest.TestCase):
    def test_clean_receipt_passes(self) -> None:
        public_access.verify_receipt(
            valid_receipt(),
            expected_repository=REPOSITORY,
            expected_release_tag=TAG,
            expected_commit_sha=COMMIT,
        )

    def test_wrong_owner_repo_fails(self) -> None:
        receipt = valid_receipt()
        receipt["repository"] = "someone/else"
        self.assert_fails(receipt, "wrong owner/repo")

    def test_auth_token_usage_fails(self) -> None:
        receipt = valid_receipt()
        receipt["auth"]["GH_TOKEN"] = True
        self.assert_fails(receipt, "used auth")

    def test_auth_required_response_fails(self) -> None:
        receipt = valid_receipt()
        receipt["downloaded_assets"][0]["http_status"] = 403
        self.assert_fails(receipt, "private or auth-required")

    def test_missing_public_asset_fails(self) -> None:
        receipt = valid_receipt()
        receipt["downloaded_assets"] = receipt["downloaded_assets"][1:]
        receipt["downloaded_asset_digests"].pop("SHA256SUMS")
        self.assert_fails(receipt, "missing SHA256SUMS")

    def test_extra_public_asset_fails(self) -> None:
        receipt = valid_receipt()
        extra = copy.deepcopy(receipt["downloaded_assets"][0])
        extra["name"] = "extra.txt"
        extra["url"] = f"https://github.com/{REPOSITORY}/releases/download/{TAG}/extra.txt"
        extra["final_url"] = extra["url"]
        receipt["downloaded_assets"].append(extra)
        receipt["downloaded_asset_digests"]["extra.txt"] = extra["sha256"]
        self.assert_fails(receipt, "extra extra.txt")

    def test_digest_mismatch_fails(self) -> None:
        receipt = valid_receipt()
        receipt["downloaded_assets"][0]["sha256"] = "b" * 64
        receipt["downloaded_asset_digests"][receipt["downloaded_assets"][0]["name"]] = "b" * 64
        self.assert_fails(receipt, "sha256 mismatch")

    def test_stale_tag_fails(self) -> None:
        receipt = valid_receipt()
        receipt["release_tag"] = "v0.2.6"
        self.assert_fails(receipt, "stale release tag")

    def test_stale_commit_fails(self) -> None:
        receipt = valid_receipt()
        receipt["commit_sha"] = "0" * 40
        self.assert_fails(receipt, "stale commit")

    def test_unsupported_schema_fails(self) -> None:
        receipt = valid_receipt()
        receipt["schema_version"] = 99
        self.assert_fails(receipt, "unsupported")

    def test_unreachable_api_fails(self) -> None:
        receipt = valid_receipt()
        receipt["api_checks"]["latest_release"]["error"] = "timed out"
        self.assert_fails(receipt, "unreachable")

    def test_latest_pinned_tag_disagreement_fails(self) -> None:
        receipt = valid_receipt()
        receipt["resolved_latest_tag"] = "v0.2.6"
        self.assert_fails(receipt, "stale tag")

    def test_latest_pinned_digest_disagreement_fails(self) -> None:
        receipt = valid_receipt()
        receipt["installer_downloads"]["latest_install"]["sha256"] = "c" * 64
        self.assert_fails(receipt, "digest disagreement")

    def assert_fails(self, receipt: dict, text: str) -> None:
        with self.assertRaises(public_access.VerificationError) as raised:
            public_access.verify_receipt(
                receipt,
                expected_repository=REPOSITORY,
                expected_release_tag=TAG,
                expected_commit_sha=COMMIT,
            )
        self.assertIn(text, str(raised.exception))


def valid_receipt() -> dict:
    assets = []
    digests = {}
    subjects = []
    for index, name in enumerate(sorted(public_access.REQUIRED_PUBLIC_ASSETS)):
        digest = INSTALL_SHA if name == "install.sh" else f"{index:064x}"[-64:]
        row = {
            "name": name,
            "url": f"https://github.com/{REPOSITORY}/releases/download/{TAG}/{name}",
            "http_status": 200,
            "final_url": f"https://github.com/{REPOSITORY}/releases/download/{TAG}/{name}",
            "size_bytes": 100 + index,
            "sha256": digest,
            "error": None,
        }
        if name in {"install.sh", "SHA256SUMS", "m80-release-assets.json"}:
            row["expected_sha256"] = digest
            row["expected_size_bytes"] = row["size_bytes"]
            subjects.append(
                {
                    "name": name,
                    "kind": "installer" if name == "install.sh" else "manifest",
                    "sha256": digest,
                    "size_bytes": row["size_bytes"],
                }
            )
        assets.append(row)
        digests[name] = digest
    latest_install = {
        "name": "install.sh",
        "url": f"https://github.com/{REPOSITORY}/releases/latest/download/install.sh",
        "http_status": 200,
        "final_url": f"https://github.com/{REPOSITORY}/releases/download/{TAG}/install.sh",
        "size_bytes": 123,
        "sha256": INSTALL_SHA,
        "error": None,
    }
    pinned_install = dict(latest_install)
    pinned_install["url"] = f"https://github.com/{REPOSITORY}/releases/download/{TAG}/install.sh"
    return {
        "schema_version": 1,
        "kind": "m80_release_readiness_public_access",
        "lane_id": "public-access-latest",
        "proof_kind": "public-access-proof",
        "status": "passed",
        "repository": REPOSITORY,
        "release_tag": TAG,
        "commit_sha": COMMIT,
        "github_release_url": f"https://github.com/{REPOSITORY}/releases/tag/{TAG}",
        "verification_time": "2026-05-22T02:00:00Z",
        "substrate": {"kind": "public-github", "fixture": False},
        "auth": {
            "GH_TOKEN": False,
            "GITHUB_TOKEN": False,
            "gh_auth_present": False,
            "authorization_header_used": False,
        },
        "latest_install_url": f"https://github.com/{REPOSITORY}/releases/latest/download/install.sh",
        "pinned_install_url": f"https://github.com/{REPOSITORY}/releases/download/{TAG}/install.sh",
        "resolved_latest_tag": TAG,
        "expected_stable_tag": TAG,
        "latest_pinned_agree": True,
        "api_checks": {
            "latest_release": {
                "url": f"https://api.github.com/repos/{REPOSITORY}/releases/latest",
                "http_status": 200,
                "final_url": f"https://api.github.com/repos/{REPOSITORY}/releases/latest",
                "error": None,
            },
            "pinned_release": {
                "url": f"https://api.github.com/repos/{REPOSITORY}/releases/tags/{TAG}",
                "http_status": 200,
                "final_url": f"https://api.github.com/repos/{REPOSITORY}/releases/tags/{TAG}",
                "error": None,
            },
        },
        "installer_downloads": {
            "latest_install": latest_install,
            "pinned_install": pinned_install,
        },
        "asset_manifest": {
            "name": "m80-release-assets.json",
            "sha256": digests["m80-release-assets.json"],
            "size_bytes": next(row["size_bytes"] for row in assets if row["name"] == "m80-release-assets.json"),
        },
        "downloaded_assets": assets,
        "downloaded_asset_digests": digests,
        "integrity_subjects": subjects,
        "release_build": {
            "source_commit": COMMIT,
            "release_tag": TAG,
            "builder_identity": f"github-actions:https://github.com/{REPOSITORY}/actions/runs/26263525140/attempts/1",
        },
    }


if __name__ == "__main__":
    unittest.main(verbosity=2)
