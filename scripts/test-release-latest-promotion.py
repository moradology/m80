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


class Paths:
    def __init__(self, root: Path, release_tag: str) -> None:
        self.root = root
        self.release_tag = release_tag
        self.release_list = root / "releases.json"
        self.publish_decision = root / "m80-release-publish-decision.json"
        self.proof_ledger = root / "m80-release-proof-ledger.jsonl"
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


def digest(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


if __name__ == "__main__":
    unittest.main()
