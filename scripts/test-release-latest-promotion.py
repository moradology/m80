#!/usr/bin/env python3
"""Tests for scripts/release_latest_promotion.py."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "release_latest_promotion.py"


class ReleaseLatestPromotionTest(unittest.TestCase):
    def test_highest_stable_target_is_approved(self) -> None:
        with fixture(release_tag="v1.2.4") as paths:
            paths.release_list.write_text(json.dumps(releases("v1.2.3", "v1.2.4")))

            result = run_promotion(paths)

            self.assertEqual(result.returncode, 0, result.stderr)
            decision = json.loads(paths.out.read_text())
            self.assertEqual(decision["decision"], "approved")
            self.assertEqual(decision["highest_stable_public_tag"], "v1.2.4")
            self.assertEqual(decision["remote_inventory_digest"], digest(paths.remote_inventory))

    def test_wrapped_release_list_is_accepted(self) -> None:
        with fixture(release_tag="v1.2.4") as paths:
            paths.release_list.write_text(json.dumps({"releases": releases("v1.2.3", "v1.2.4")}))

            result = run_promotion(paths)

            self.assertEqual(result.returncode, 0, result.stderr)
            decision = json.loads(paths.out.read_text())
            self.assertEqual(decision["decision"], "approved")
            self.assertEqual(decision["highest_stable_public_tag"], "v1.2.4")

    def test_lower_stable_target_is_refused_without_rollback_receipt(self) -> None:
        with fixture(release_tag="v1.2.3") as paths:
            paths.release_list.write_text(json.dumps(releases("v1.2.3", "v1.2.4")))

            result = run_promotion(paths)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("target is older than highest public stable release", result.stderr)
            decision = json.loads(paths.out.read_text())
            self.assertEqual(decision["decision"], "refused")
            self.assertEqual(decision["highest_stable_public_tag"], "v1.2.4")
            self.assertIn("rollback-receipt", decision["remediation"])

    def test_explicit_rollback_receipt_allows_lower_stable_target(self) -> None:
        with fixture(release_tag="v1.2.3") as paths:
            paths.release_list.write_text(json.dumps(releases("v1.2.3", "v1.2.4")))
            write_rollback_receipt(paths, highest="v1.2.4")

            result = run_promotion(paths, rollback=True)

            self.assertEqual(result.returncode, 0, result.stderr)
            decision = json.loads(paths.out.read_text())
            self.assertEqual(decision["decision"], "rollback_approved")
            self.assertEqual(decision["rollback_receipt_digest"], digest(paths.rollback))

    def test_draft_and_prerelease_tags_do_not_define_highest_stable(self) -> None:
        with fixture(release_tag="v1.2.3") as paths:
            paths.release_list.write_text(
                json.dumps(
                    [
                        release("v1.2.3"),
                        release("v1.2.4", draft=True),
                        release("v1.2.5", prerelease=True),
                        release("v1.2.6-rc.1"),
                    ]
                )
            )

            result = run_promotion(paths)

            self.assertEqual(result.returncode, 0, result.stderr)
            decision = json.loads(paths.out.read_text())
            self.assertEqual(decision["highest_stable_public_tag"], "v1.2.3")

    def test_stale_rollback_receipt_digest_is_refused(self) -> None:
        with fixture(release_tag="v1.2.3") as paths:
            paths.release_list.write_text(json.dumps(releases("v1.2.3", "v1.2.4")))
            write_rollback_receipt(paths, highest="v1.2.4", proof_ledger_digest="sha256:" + "0" * 64)

            result = run_promotion(paths, rollback=True)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("rollback receipt is invalid", result.stderr)
            decision = json.loads(paths.out.read_text())
            self.assertEqual(decision["decision"], "refused")
            self.assertIn("proof_ledger_digest mismatch", decision["reason"])

    def test_missing_rollback_receipt_field_is_refused(self) -> None:
        with fixture(release_tag="v1.2.3") as paths:
            paths.release_list.write_text(json.dumps(releases("v1.2.3", "v1.2.4")))
            write_rollback_receipt(paths, highest="v1.2.4")
            payload = json.loads(paths.rollback.read_text())
            payload.pop("actor")
            paths.rollback.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")

            result = run_promotion(paths, rollback=True)

            self.assertNotEqual(result.returncode, 0)
            decision = json.loads(paths.out.read_text())
            self.assertEqual(decision["decision"], "refused")
            self.assertIn("missing fields: actor", decision["reason"])

    def test_malformed_rollback_receipt_timestamp_is_refused(self) -> None:
        with fixture(release_tag="v1.2.3") as paths:
            paths.release_list.write_text(json.dumps(releases("v1.2.3", "v1.2.4")))
            write_rollback_receipt(paths, highest="v1.2.4")
            payload = json.loads(paths.rollback.read_text())
            payload["generated_at"] = "not-a-timestamp"
            paths.rollback.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")

            result = run_promotion(paths, rollback=True)

            self.assertNotEqual(result.returncode, 0)
            decision = json.loads(paths.out.read_text())
            self.assertEqual(decision["decision"], "refused")
            self.assertIn("generated_at must be an ISO-8601 timestamp", decision["reason"])

    def test_missing_remote_inventory_is_refused(self) -> None:
        with fixture(release_tag="v1.2.4") as paths:
            paths.remote_inventory.unlink()

            result = run_promotion(paths)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("remote release asset inventory missing", result.stderr)

    def test_stale_remote_inventory_is_refused(self) -> None:
        with fixture(release_tag="v1.2.4") as paths:
            assets = public_assets()
            assets[0] = dict(assets[0], sha256="0" * 64)
            write_remote_inventory(paths, assets=assets)

            result = run_promotion(paths)

            self.assertNotEqual(result.returncode, 0)
            decision = json.loads(paths.out.read_text())
            self.assertEqual(decision["decision"], "refused")
            self.assertIn("remote inventory install.sh sha256 mismatch", decision["reason"])

    def test_extra_remote_inventory_asset_is_refused(self) -> None:
        with fixture(release_tag="v1.2.4") as paths:
            assets = public_assets()
            assets.append(
                {
                    "name": "unexpected.txt",
                    "kind": "debug",
                    "sha256": "3" * 64,
                    "size_bytes": 1,
                    "integrity_subject": False,
                }
            )
            write_remote_inventory(paths, assets=assets)

            result = run_promotion(paths)

            self.assertNotEqual(result.returncode, 0)
            decision = json.loads(paths.out.read_text())
            self.assertIn("remote inventory extra asset(s): unexpected.txt", decision["reason"])

    def test_duplicate_remote_inventory_name_is_refused(self) -> None:
        with fixture(release_tag="v1.2.4") as paths:
            inventory = json.loads(paths.remote_inventory.read_text())
            duplicate = dict(inventory["assets"][0])
            duplicate["id"] = 9999
            inventory["assets"].append(duplicate)
            paths.remote_inventory.write_text(json.dumps(inventory, indent=2, sort_keys=True) + "\n")

            result = run_promotion(paths)

            self.assertNotEqual(result.returncode, 0)
            decision = json.loads(paths.out.read_text())
            self.assertIn("remote inventory duplicate asset name: install.sh", decision["reason"])

    def test_remote_inventory_timestamp_drift_is_diagnostic(self) -> None:
        with fixture(release_tag="v1.2.4") as paths:
            inventory = json.loads(paths.remote_inventory.read_text())
            inventory["assets"][0]["updated_at"] = "2026-05-24T00:00:00Z"
            inventory["assets"][1]["created_at"] = "2026-05-25T00:00:00Z"
            paths.remote_inventory.write_text(json.dumps(inventory, indent=2, sort_keys=True) + "\n")

            result = run_promotion(paths)

            self.assertEqual(result.returncode, 0, result.stderr)
            decision = json.loads(paths.out.read_text())
            self.assertEqual(decision["decision"], "approved")

    def test_remote_inventory_malformed_timestamp_is_refused(self) -> None:
        with fixture(release_tag="v1.2.4") as paths:
            inventory = json.loads(paths.remote_inventory.read_text())
            inventory["assets"][0]["updated_at"] = "not-a-timestamp"
            paths.remote_inventory.write_text(json.dumps(inventory, indent=2, sort_keys=True) + "\n")

            result = run_promotion(paths)

            self.assertNotEqual(result.returncode, 0)
            decision = json.loads(paths.out.read_text())
            self.assertIn("remote inventory assets[0] updated_at must be an ISO-8601 timestamp", decision["reason"])


class Paths:
    def __init__(self, root: Path, release_tag: str) -> None:
        self.root = root
        self.release_tag = release_tag
        self.release_list = root / "releases.json"
        self.publish_decision = root / "m80-release-publish-decision.json"
        self.proof_ledger = root / "m80-release-proof-ledger.jsonl"
        self.upload_manifest = root / "m80-release-upload-manifest.json"
        self.remote_inventory = root / "m80-release-remote-assets.json"
        self.rollback = root / "release-latest-rollback-receipt.json"
        self.out = root / "m80-latest-promotion-decision.json"


class fixture:
    def __init__(self, *, release_tag: str) -> None:
        self.release_tag = release_tag
        self.tempdir: tempfile.TemporaryDirectory[str] | None = None

    def __enter__(self) -> Paths:
        self.tempdir = tempfile.TemporaryDirectory()
        paths = Paths(Path(self.tempdir.name), self.release_tag)
        paths.release_list.write_text(json.dumps(releases(self.release_tag)))
        paths.publish_decision.write_text(json.dumps({"release_tag": self.release_tag, "decision": "approved"}))
        paths.proof_ledger.write_text(json.dumps({"release_tag": self.release_tag}) + "\n")
        write_upload_manifest(paths)
        write_remote_inventory(paths)
        return paths

    def __exit__(self, *args: object) -> None:
        assert self.tempdir is not None
        self.tempdir.cleanup()


def run_promotion(paths: Paths, *, rollback: bool = False) -> subprocess.CompletedProcess[str]:
    args = [
        "python3",
        str(SCRIPT),
        "--release-tag",
        paths.release_tag,
        "--release-list",
        str(paths.release_list),
        "--publish-decision",
        str(paths.publish_decision),
        "--proof-ledger",
        str(paths.proof_ledger),
        "--upload-manifest",
        str(paths.upload_manifest),
        "--remote-inventory",
        str(paths.remote_inventory),
        "--out",
        str(paths.out),
        "--generated-at",
        "2026-05-23T00:00:00Z",
        "--write",
    ]
    if rollback:
        args.extend(["--rollback-receipt", str(paths.rollback)])
    return subprocess.run(args, cwd=REPO_ROOT, text=True, capture_output=True, check=False)


def write_rollback_receipt(
    paths: Paths,
    *,
    highest: str,
    proof_ledger_digest: str | None = None,
) -> None:
    payload = {
        "schema_version": 1,
        "kind": "m80_release_latest_rollback_receipt",
        "decision": "approved",
        "release_tag": paths.release_tag,
        "highest_stable_public_tag": highest,
        "reason": "operator rollback to restore a known-good release",
        "actor": "maintainer",
        "publish_decision_digest": digest(paths.publish_decision),
        "proof_ledger_digest": proof_ledger_digest or digest(paths.proof_ledger),
        "generated_at": "2026-05-23T00:00:00Z",
    }
    paths.rollback.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")


def releases(*tags: str) -> list[dict[str, object]]:
    return [release(tag) for tag in tags]


def release(tag: str, *, draft: bool = False, prerelease: bool = False) -> dict[str, object]:
    return {"tagName": tag, "isDraft": draft, "isPrerelease": prerelease}


def public_assets() -> list[dict[str, object]]:
    return [
        {
            "name": "install.sh",
            "kind": "installer",
            "sha256": "1" * 64,
            "size_bytes": 123,
            "integrity_subject": True,
        },
        {
            "name": "m80-linux-x86_64.tar.gz",
            "kind": "release-bundle",
            "sha256": "2" * 64,
            "size_bytes": 456,
            "integrity_subject": True,
        },
    ]


def write_upload_manifest(paths: Paths, *, assets: list[dict[str, object]] | None = None) -> None:
    paths.upload_manifest.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "release_tag": paths.release_tag,
                "public_assets": assets if assets is not None else public_assets(),
            },
            indent=2,
            sort_keys=True,
        )
        + "\n"
    )


def write_remote_inventory(paths: Paths, *, assets: list[dict[str, object]] | None = None) -> None:
    rows = []
    for index, asset in enumerate(assets if assets is not None else public_assets(), start=1):
        rows.append(
            {
                "id": 1000 + index,
                "name": asset["name"],
                "kind": asset["kind"],
                "sha256": asset["sha256"],
                "size_bytes": asset["size_bytes"],
                "download_url": f"https://github.com/moradology/m80/releases/download/{paths.release_tag}/{asset['name']}",
                "created_at": "2026-05-23T00:00:00Z",
                "updated_at": "2026-05-23T00:00:01Z",
            }
        )
    paths.remote_inventory.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "kind": "m80_release_remote_asset_inventory",
                "release_tag": paths.release_tag,
                "release_id": 9001,
                "generated_at": "2026-05-23T00:00:00Z",
                "assets": rows,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n"
    )


def digest(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


if __name__ == "__main__":
    unittest.main()
