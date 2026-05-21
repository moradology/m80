#!/usr/bin/env python3
"""Tests for scripts/render-freshness-status.py."""

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
RENDER = REPO_ROOT / "scripts" / "render-freshness-status.py"


class FreshnessStatusRenderTest(unittest.TestCase):
    def test_committed_docs_status_markers_are_generated(self) -> None:
        result = run_render(REPO_ROOT, REPO_ROOT / "docs/behaviors/release/freshness-status-docs.json", check=True)

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_pending_status_renders_without_changing_quickstart_command(self) -> None:
        with render_fixture(status="pending") as fixture:
            original = fixture.readme.read_text()

            result = run_render(fixture.root, fixture.status)

            self.assertEqual(result.returncode, 0, result.stderr)
            rendered = fixture.readme.read_text()
            self.assertIn("Public installer status: pending public proof.", rendered)
            self.assertIn(latest_install_command(), rendered)
            self.assertIn(latest_install_command(), original)

    def test_public_green_status_renders_public_proof_link(self) -> None:
        with render_fixture(status="public_green") as fixture:
            result = run_render(fixture.root, fixture.status)

            self.assertEqual(result.returncode, 0, result.stderr)
            rendered = fixture.readme.read_text()
            self.assertIn("public proof green for `v1.2.3`", rendered)
            self.assertIn("[latest-and-pinned-url-proof](public-proof.json)", rendered)

    def test_scaffolded_status_renders_fixture_not_public(self) -> None:
        with render_fixture(status="scaffolded", substrate="fixture-hostless", artifact_class="fixture") as fixture:
            result = run_render(fixture.root, fixture.status)

            self.assertEqual(result.returncode, 0, result.stderr)
            rendered = fixture.readme.read_text()
            self.assertIn("scaffolded fixture proof", rendered)
            self.assertIn("This is not public release proof", rendered)

    def test_failed_status_renders_previous_public_green_link(self) -> None:
        with render_fixture(
            status="failed",
            proof_kind="previous-public-green-status",
            install_url_proofs=False,
            public_assets=False,
        ) as fixture:
            result = run_render(fixture.root, fixture.status)

            self.assertEqual(result.returncode, 0, result.stderr)
            rendered = fixture.readme.read_text()
            self.assertIn("Public installer status: failed", rendered)
            self.assertIn("Previous public green: [previous-public-green-status](public-proof.json)", rendered)

    def test_stale_status_renders_expected_tag(self) -> None:
        with render_fixture(status="stale", expected_tag="v1.2.4", install_url_proofs=False, public_assets=False) as fixture:
            result = run_render(fixture.root, fixture.status)

            self.assertEqual(result.returncode, 0, result.stderr)
            rendered = fixture.readme.read_text()
            self.assertIn("Public installer status: stale for `v1.2.3`; expected `v1.2.4`", rendered)

    def test_manual_marker_edit_fails_check(self) -> None:
        with render_fixture(status="pending") as fixture:
            fixture.readme.write_text(
                fixture.readme.read_text().replace(
                    "Public installer status: pending public proof.",
                    "Public installer status: pending public proof. Edited by hand.",
                )
            )

            result = run_render(fixture.root, fixture.status, check=True)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("freshness status marker is stale", result.stdout)

    def test_missing_proof_artifact_fails(self) -> None:
        with render_fixture(status="public_green") as fixture:
            (fixture.root / "public-proof.json").unlink()

            result = run_render(fixture.root, fixture.status, check=True)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("path is missing", result.stderr)

    def test_public_green_rejects_workflow_artifact(self) -> None:
        with render_fixture(status="public_green", artifact_class="workflow") as fixture:
            result = run_render(fixture.root, fixture.status, check=True)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("public_green proof_artifacts must be public artifacts", result.stderr)


class Fixture:
    def __init__(self, tmp: tempfile.TemporaryDirectory[str], root: Path, readme: Path, status: Path) -> None:
        self._tmp = tmp
        self.root = root
        self.readme = readme
        self.status = status

    def __enter__(self) -> "Fixture":
        return self

    def __exit__(self, *_args: object) -> None:
        self._tmp.cleanup()


def render_fixture(
    *,
    status: str,
    substrate: str = "public-unauthenticated",
    artifact_class: str = "public",
    proof_kind: str = "latest-and-pinned-url-proof",
    expected_tag: str = "v1.2.3",
    install_url_proofs: bool = True,
    public_assets: bool = True,
) -> Fixture:
    tmp = tempfile.TemporaryDirectory()
    root = Path(tmp.name)
    readme = root / "README.md"
    readme.write_text(docs_text())
    status_path = root / "m80-latest-freshness-status.json"
    write_status(
        status_path,
        root=root,
        docs_root=root,
        status=status,
        substrate=substrate,
        artifact_class=artifact_class,
        proof_kind=proof_kind,
        expected_tag=expected_tag,
        install_url_proofs=install_url_proofs,
        public_assets=public_assets,
    )
    return Fixture(tmp, root, readme, status_path)


def docs_text() -> str:
    return textwrap.dedent(
        f"""
        # m80 fixture

        <!-- m80:freshness-status start -->
        Public installer status: pending public proof.
        <!-- m80:freshness-status end -->

        <!-- m80:quickstart-snippet latest-install start -->
        ```sh
        {latest_install_command()}
        ```
        <!-- m80:quickstart-snippet latest-install end -->
        """
    ).lstrip()


def write_status(
    path: Path,
    *,
    root: Path,
    docs_root: Path,
    status: str,
    substrate: str,
    artifact_class: str,
    proof_kind: str,
    expected_tag: str,
    install_url_proofs: bool,
    public_assets: bool,
) -> None:
    release_root = public_release_root()
    proof = root / "public-proof.json"
    proof.write_text('{"proof": "latest and pinned public URLs were fetched"}\n')
    proof_artifacts = [
        {
            "kind": proof_kind,
            "path": proof.name,
            "sha256": freshness_status.sha256_ref(proof),
            "artifact_class": artifact_class,
        }
    ]
    url_proofs = [
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
    assets = [
        {
            "name": name,
            "role": public_asset_role(name),
            "url": release_root.asset_url("v1.2.3", name),
            "sha256": ("%064x" % (index + 1)),
            "size_bytes": index + 1,
        }
        for index, name in enumerate(sorted(REQUIRED_PUBLIC_ASSETS))
    ]
    if status == "pending":
        substrate = "none"
        proof_artifacts = []
        url_proofs = []
        assets = []
    if not install_url_proofs:
        url_proofs = []
    if not public_assets:
        assets = []
    payload = {
        "schema_version": freshness_status.SCHEMA_VERSION,
        "generated_at": "2026-05-21T21:00:00Z",
        "status": status,
        "owner": release_root.owner,
        "repo": release_root.repo,
        "resolved_latest_tag": "v1.2.3",
        "expected_highest_stable_tag": expected_tag,
        "latest_install_url": release_root.latest_install_url,
        "pinned_install_url": release_root.pinned_install_url("v1.2.3"),
        "proof_artifacts": proof_artifacts,
        "proof_substrate": substrate,
        "workflow_run_id": "123456789",
        "source_commit": "a" * 40,
        "checked_command_inventory_digest": freshness_status.command_inventory_digest(docs_root),
        "install_url_proofs": url_proofs,
        "public_assets": assets,
    }
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")


def run_render(root: Path, status: Path, *, check: bool = False) -> subprocess.CompletedProcess[str]:
    cmd = [
        "python3",
        str(RENDER),
        "--docs-root",
        str(root),
        "--status",
        str(status),
        "--artifact-root",
        str(status.parent),
        "--doc",
        "README.md",
    ]
    if root == REPO_ROOT:
        cmd = ["python3", str(RENDER), "--check"]
    elif check:
        cmd.append("--check")
    return subprocess.run(
        cmd,
        cwd=REPO_ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


if __name__ == "__main__":
    unittest.main()
