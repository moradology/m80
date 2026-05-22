#!/usr/bin/env python3
"""Tests for scripts/verify-freshness-status.py."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest

import freshness_status
from release_url_contract import latest_install_command, public_release_root
from stable_release_channel import REQUIRED_PUBLIC_ASSETS, public_asset_role


REPO_ROOT = Path(__file__).resolve().parents[1]
VERIFY = REPO_ROOT / "scripts" / "verify-freshness-status.py"


class FreshnessStatusTest(unittest.TestCase):
    def test_valid_public_green_status_passes_and_writes_result(self) -> None:
        with status_fixture() as fixture:
            result_out = fixture.root / "freshness-status.verifier-result.json"

            result = run_verify(fixture.status, fixture.root, fixture.docs_root, result_out=result_out)

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("validated freshness status", result.stdout)
            payload = read_json(result_out)
            self.assertEqual(payload["verifier"], "verify-freshness-status.py")
            self.assertEqual(payload["status_artifact"], "m80-latest-freshness-status.json")
            self.assertEqual(payload["status"], "public_green")
            self.assertEqual(payload["resolved_latest_tag"], "v1.2.3")
            self.assertTrue(payload["passed"])

    def test_missing_required_field_fails(self) -> None:
        with status_fixture() as fixture:
            payload = read_json(fixture.status)
            del payload["workflow_run_id"]
            write_json(fixture.status, payload)

            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("missing field(s): workflow_run_id", result.stderr)

    def test_unknown_status_fails(self) -> None:
        with status_fixture() as fixture:
            payload = read_json(fixture.status)
            payload["status"] = "pretty_good"
            write_json(fixture.status, payload)

            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unknown status", result.stderr)

    def test_fixture_proof_cannot_be_labeled_public_green(self) -> None:
        with status_fixture() as fixture:
            payload = read_json(fixture.status)
            payload["proof_substrate"] = "fixture-hostless"
            payload["proof_artifacts"][0]["artifact_class"] = "fixture"
            write_json(fixture.status, payload)

            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("fixture proof can appear only as scaffolded", result.stderr)

    def test_fixture_only_scaffolded_status_passes(self) -> None:
        with status_fixture(status="scaffolded", substrate="fixture-hostless", artifact_class="fixture") as fixture:
            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertEqual(result.returncode, 0, result.stderr)

    def test_pending_status_has_no_proof_material(self) -> None:
        with status_fixture(status="pending", substrate="none", proof_material=False) as fixture:
            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertEqual(result.returncode, 0, result.stderr)

    def test_pending_status_rejects_proof_material(self) -> None:
        with status_fixture(status="pending", substrate="none") as fixture:
            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("pending must not reference proof_artifacts", result.stderr)

    def test_pending_status_requires_none_substrate(self) -> None:
        with status_fixture(status="pending", substrate="public-unauthenticated", proof_material=False) as fixture:
            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("pending requires proof_substrate=none", result.stderr)

    def test_stale_command_inventory_digest_fails(self) -> None:
        with status_fixture() as fixture:
            payload = read_json(fixture.status)
            payload["checked_command_inventory_digest"] = "sha256:" + ("0" * 64)
            write_json(fixture.status, payload)

            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("checked_command_inventory_digest is stale", result.stderr)

    def test_nondeterministic_asset_order_fails(self) -> None:
        with status_fixture() as fixture:
            payload = read_json(fixture.status)
            payload["public_assets"] = list(reversed(payload["public_assets"]))
            write_json(fixture.status, payload)

            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("public_assets must be sorted by name", result.stderr)

    def test_public_green_requires_latest_and_pinned_unauthenticated_proofs(self) -> None:
        with status_fixture() as fixture:
            payload = read_json(fixture.status)
            payload["install_url_proofs"] = [
                row for row in payload["install_url_proofs"] if row["kind"] == "latest-install"
            ]
            write_json(fixture.status, payload)

            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("requires latest and pinned install proofs", result.stderr)

    def test_public_assets_bind_to_resolved_release_url(self) -> None:
        with status_fixture() as fixture:
            payload = read_json(fixture.status)
            payload["public_assets"][0]["url"] = "https://github.com/moradology/m80/releases/download/v9.9.9/SHA256SUMS"
            write_json(fixture.status, payload)

            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("url mismatch for asset", result.stderr)

    def test_valid_empty_safety_floor_passes(self) -> None:
        with status_fixture() as fixture:
            payload = read_json(fixture.status)

            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                payload["safety_floor"],
                {
                    "schema_version": 1,
                    "published_at": "2026-05-21T21:00:00Z",
                    "minimum_safe_tag": None,
                    "yanked_releases": [],
                },
            )

    def test_valid_yanked_safety_floor_passes(self) -> None:
        with status_fixture() as fixture:
            payload = read_json(fixture.status)
            payload["safety_floor"] = safety_floor(
                minimum_safe_tag=None,
                yanked_releases=[
                    {
                        "tag": "v1.2.2",
                        "reason": "bad release",
                        "advisory_url": "https://github.com/moradology/m80/issues/1",
                        "issue_id": None,
                        "published_at": "2026-05-21T21:00:00Z",
                        "replacement_command": pinned_install_command("v1.2.3"),
                        "no_replacement_reason": None,
                    }
                ],
            )
            write_json(fixture.status, payload)

            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertEqual(result.returncode, 0, result.stderr)

    def test_valid_minimum_safe_safety_floor_passes(self) -> None:
        with status_fixture() as fixture:
            payload = read_json(fixture.status)
            payload["safety_floor"] = safety_floor(
                minimum_safe_tag={
                    "tag": "v1.2.0",
                    "reason": "security floor",
                    "advisory_url": None,
                    "issue_id": "m80-o3uh9.21.9",
                    "replacement_command": pinned_install_command("v1.2.3"),
                },
                yanked_releases=[],
            )
            write_json(fixture.status, payload)

            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertEqual(result.returncode, 0, result.stderr)

    def test_safety_floor_missing_reason_fails(self) -> None:
        with status_fixture() as fixture:
            payload = read_json(fixture.status)
            payload["safety_floor"] = safety_floor(
                minimum_safe_tag={
                    "tag": "v1.2.0",
                    "advisory_url": None,
                    "issue_id": "m80-o3uh9.21.9",
                    "replacement_command": pinned_install_command("v1.2.3"),
                },
                yanked_releases=[],
            )
            write_json(fixture.status, payload)

            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("missing field(s): reason", result.stderr)

    def test_safety_floor_malformed_published_at_fails(self) -> None:
        with status_fixture() as fixture:
            payload = read_json(fixture.status)
            payload["safety_floor"]["published_at"] = "2026-05-21 21:00:00"
            write_json(fixture.status, payload)

            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("published_at must be RFC3339 UTC seconds", result.stderr)

    def test_safety_floor_bad_replacement_command_fails(self) -> None:
        with status_fixture() as fixture:
            payload = read_json(fixture.status)
            payload["safety_floor"] = safety_floor(
                minimum_safe_tag={
                    "tag": "v1.2.0",
                    "reason": "security floor",
                    "advisory_url": None,
                    "issue_id": "m80-o3uh9.21.9",
                    "replacement_command": latest_install_command(),
                },
                yanked_releases=[],
            )
            write_json(fixture.status, payload)

            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("replacement_command must be a pinned install.sh command", result.stderr)

    def test_yanked_release_requires_replacement_or_no_replacement_reason(self) -> None:
        with status_fixture() as fixture:
            payload = read_json(fixture.status)
            payload["safety_floor"] = safety_floor(
                minimum_safe_tag=None,
                yanked_releases=[
                    {
                        "tag": "v1.2.2",
                        "reason": "bad release",
                        "advisory_url": None,
                        "issue_id": "m80-o3uh9.21.9",
                        "published_at": "2026-05-21T21:00:00Z",
                        "replacement_command": None,
                        "no_replacement_reason": None,
                    }
                ],
            )
            write_json(fixture.status, payload)

            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("requires replacement_command or no_replacement_reason", result.stderr)

    def test_safety_floor_minimum_newer_than_latest_fails(self) -> None:
        with status_fixture() as fixture:
            payload = read_json(fixture.status)
            payload["safety_floor"] = safety_floor(
                minimum_safe_tag=minimum_safe_tag("v9.0.0", "v1.2.3"),
                yanked_releases=[],
            )
            write_json(fixture.status, payload)

            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("minimum_safe_tag tag v9.0.0", result.stderr)
            self.assertIn("resolved_latest_tag v1.2.3", result.stderr)

    def test_safety_floor_duplicate_yanked_tag_fails(self) -> None:
        with status_fixture() as fixture:
            payload = read_json(fixture.status)
            payload["safety_floor"] = safety_floor(
                minimum_safe_tag=None,
                yanked_releases=[
                    yanked_release("v1.2.2", replacement_tag="v1.2.3"),
                    yanked_release("v1.2.2", replacement_tag="v1.2.3"),
                ],
            )
            write_json(fixture.status, payload)

            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("yanked_releases[1] tag v1.2.2 duplicates", result.stderr)

    def test_safety_floor_unknown_yanked_tag_syntax_fails(self) -> None:
        with status_fixture() as fixture:
            payload = read_json(fixture.status)
            payload["safety_floor"] = safety_floor(
                minimum_safe_tag=None,
                yanked_releases=[yanked_release("v1.2.2-rc.1", replacement_tag="v1.2.3")],
            )
            write_json(fixture.status, payload)

            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("yanked_releases[0] tag must be a stable", result.stderr)

    def test_safety_floor_yanked_latest_without_replacement_command_fails(self) -> None:
        with status_fixture() as fixture:
            payload = read_json(fixture.status)
            payload["safety_floor"] = safety_floor(
                minimum_safe_tag=None,
                yanked_releases=[
                    yanked_release(
                        "v1.2.3",
                        replacement_tag=None,
                        no_replacement_reason="withdrawn with no automatic replacement",
                    )
                ],
            )
            write_json(fixture.status, payload)

            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("replacement_command for v1.2.3", result.stderr)
            self.assertIn("resolved_latest_tag", result.stderr)

    def test_safety_floor_replacement_command_cannot_target_yanked_tag(self) -> None:
        with status_fixture() as fixture:
            payload = read_json(fixture.status)
            payload["safety_floor"] = safety_floor(
                minimum_safe_tag=None,
                yanked_releases=[
                    yanked_release("v1.2.1", replacement_tag="v1.2.2"),
                    yanked_release("v1.2.2", replacement_tag="v1.2.3"),
                ],
            )
            write_json(fixture.status, payload)

            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("replacement_command for v1.2.1", result.stderr)
            self.assertIn("yanked tag v1.2.2", result.stderr)

    def test_safety_floor_replacement_command_cannot_target_below_floor(self) -> None:
        with status_fixture() as fixture:
            payload = read_json(fixture.status)
            payload["safety_floor"] = safety_floor(
                minimum_safe_tag=minimum_safe_tag("v1.2.2", "v1.2.1"),
                yanked_releases=[],
            )
            write_json(fixture.status, payload)

            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("replacement_command for v1.2.2", result.stderr)
            self.assertIn("below minimum_safe_tag v1.2.2", result.stderr)

    def test_safety_floor_published_at_after_status_generated_at_fails(self) -> None:
        with status_fixture() as fixture:
            payload = read_json(fixture.status)
            payload["safety_floor"]["published_at"] = "2026-05-21T21:00:01Z"
            write_json(fixture.status, payload)

            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("safety_floor published_at 2026-05-21T21:00:01Z", result.stderr)
            self.assertIn("generated_at 2026-05-21T21:00:00Z", result.stderr)

    def test_yanked_release_published_at_after_safety_floor_fails(self) -> None:
        with status_fixture() as fixture:
            payload = read_json(fixture.status)
            payload["safety_floor"] = safety_floor(
                minimum_safe_tag=None,
                yanked_releases=[
                    yanked_release(
                        "v1.2.2",
                        replacement_tag="v1.2.3",
                        published_at="2026-05-21T21:00:01Z",
                    )
                ],
            )
            write_json(fixture.status, payload)

            result = run_verify(fixture.status, fixture.root, fixture.docs_root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("yanked_releases[0] published_at for v1.2.2 2026-05-21T21:00:01Z", result.stderr)
            self.assertIn("safety_floor.published_at 2026-05-21T21:00:00Z", result.stderr)


class Fixture:
    def __init__(self, tmp: tempfile.TemporaryDirectory[str], root: Path, docs_root: Path, status: Path) -> None:
        self._tmp = tmp
        self.root = root
        self.docs_root = docs_root
        self.status = status

    def __enter__(self) -> "Fixture":
        return self

    def __exit__(self, *_args: object) -> None:
        self._tmp.cleanup()


def status_fixture(
    *,
    status: str = "public_green",
    substrate: str = "public-unauthenticated",
    artifact_class: str = "public",
    proof_material: bool = True,
) -> Fixture:
    tmp = tempfile.TemporaryDirectory()
    root = Path(tmp.name)
    docs_root = write_docs_root(root / "docs")
    proof = root / "public-proof.json"
    proof.write_text('{"proof": "latest and pinned public URLs were fetched"}\n')
    status_path = root / "m80-latest-freshness-status.json"
    release_root = public_release_root()
    proof_artifacts = [
        {
            "kind": "latest-and-pinned-url-proof",
            "path": proof.name,
            "sha256": freshness_status.sha256_ref(proof),
            "artifact_class": artifact_class,
        }
    ]
    install_url_proofs = [
        {
            "kind": "latest-install",
            "url": release_root.latest_install_url,
            "final_url": release_root.pinned_install_url("v1.2.3"),
            "http_status": 200,
            "unauthenticated": True,
            "artifact_path": proof.name,
        },
        {
            "kind": "pinned-install",
            "url": release_root.pinned_install_url("v1.2.3"),
            "final_url": release_root.pinned_install_url("v1.2.3"),
            "http_status": 200,
            "unauthenticated": True,
            "artifact_path": proof.name,
        },
    ]
    public_assets = [
        {
            "name": name,
            "role": public_asset_role(name),
            "url": release_root.asset_url("v1.2.3", name),
            "sha256": ("%064x" % (index + 1)),
            "size_bytes": index + 1,
        }
        for index, name in enumerate(sorted(REQUIRED_PUBLIC_ASSETS))
    ]
    if not proof_material:
        proof_artifacts = []
        install_url_proofs = []
        public_assets = []
    payload = {
        "schema_version": freshness_status.SCHEMA_VERSION,
        "generated_at": "2026-05-21T21:00:00Z",
        "status": status,
        "owner": release_root.owner,
        "repo": release_root.repo,
        "resolved_latest_tag": "v1.2.3",
        "expected_highest_stable_tag": "v1.2.3",
        "latest_install_url": release_root.latest_install_url,
        "pinned_install_url": release_root.pinned_install_url("v1.2.3"),
        "proof_artifacts": proof_artifacts,
        "proof_substrate": substrate,
        "workflow_run_id": "123456789",
        "source_commit": "a" * 40,
        "checked_command_inventory_digest": freshness_status.command_inventory_digest(docs_root),
        "install_url_proofs": install_url_proofs,
        "public_assets": public_assets,
        "safety_floor": safety_floor(minimum_safe_tag=None, yanked_releases=[]),
    }
    write_json(status_path, payload)
    return Fixture(tmp, root, docs_root, status_path)


def write_docs_root(root: Path) -> Path:
    root.mkdir(parents=True)
    (root / "README.md").write_text(
        textwrap.dedent(
            f"""
            # m80 fixture docs

            <!-- m80:freshness-status start -->
            Public installer status: pending public proof.
            <!-- m80:freshness-status end -->

            ```sh
            {latest_install_command()}
            ```
            """
        ).lstrip()
    )
    return root


def safety_floor(*, minimum_safe_tag: dict | None, yanked_releases: list[dict]) -> dict:
    return {
        "schema_version": 1,
        "published_at": "2026-05-21T21:00:00Z",
        "minimum_safe_tag": minimum_safe_tag,
        "yanked_releases": yanked_releases,
    }


def minimum_safe_tag(tag: str, replacement_tag: str) -> dict:
    return {
        "tag": tag,
        "reason": "security floor",
        "advisory_url": None,
        "issue_id": "m80-o3uh9.21.9",
        "replacement_command": pinned_install_command(replacement_tag),
    }


def yanked_release(
    tag: str,
    *,
    replacement_tag: str | None,
    no_replacement_reason: str | None = None,
    published_at: str = "2026-05-21T21:00:00Z",
) -> dict:
    return {
        "tag": tag,
        "reason": "bad release",
        "advisory_url": "https://github.com/moradology/m80/issues/1",
        "issue_id": None,
        "published_at": published_at,
        "replacement_command": pinned_install_command(replacement_tag) if replacement_tag is not None else None,
        "no_replacement_reason": no_replacement_reason,
    }


def pinned_install_command(tag: str) -> str:
    return f"curl -fsSL {public_release_root().pinned_install_url(tag)} | sudo sh"


def run_verify(
    status: Path,
    artifact_root: Path,
    docs_root: Path,
    *,
    result_out: Path | None = None,
) -> subprocess.CompletedProcess[str]:
    cmd = [
        "python3",
        str(VERIFY),
        str(status),
        "--artifact-root",
        str(artifact_root),
        "--docs-root",
        str(docs_root),
    ]
    if result_out is not None:
        cmd.extend(["--result-out", str(result_out)])
    return subprocess.run(
        cmd,
        cwd=REPO_ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


def read_json(path: Path) -> dict:
    return json.loads(path.read_text())


def write_json(path: Path, payload: dict) -> None:
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    unittest.main()
