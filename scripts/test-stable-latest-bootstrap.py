#!/usr/bin/env python3
"""Tests for stable latest bootstrap resolution."""

from __future__ import annotations

import json
from pathlib import Path
import stat
import subprocess
import tempfile
import textwrap
import unittest

from release_url_contract import release_asset_url
from stable_latest_bootstrap import MetadataSource, resolve_latest_bootstrap
from stable_release_channel import (
    BUNDLE_NAME,
    CHECKSUM_NAME,
    INTEGRITY_ATTESTATION_BUNDLE_NAME,
    METADATA_NAME,
    REQUIRED_PUBLIC_ASSETS,
)


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "stable_latest_bootstrap.py"


class StableLatestBootstrapTest(unittest.TestCase):
    def test_resolves_latest_once_and_renders_only_pinned_asset_urls(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            latest = write_json(root / "latest.json", base_release_metadata())
            guard = write_json(root / "guard.json", base_release_metadata())
            index = write_json(root / "assets.json", base_asset_index())

            result = subprocess.run(
                [
                    "python3",
                    str(SCRIPT),
                    "--latest-metadata",
                    str(latest),
                    "--guard-metadata",
                    str(guard),
                    "--asset-index",
                    str(index),
                    "--json",
                ],
                cwd=REPO_ROOT,
                check=True,
                text=True,
                capture_output=True,
            )

        payload = json.loads(result.stdout)
        self.assertTrue(payload["stable_latest_resolved"])
        self.assertEqual(payload["resolved_tag"], "v1.2.3")
        self.assertEqual(payload["install_url"], release_asset_url("v1.2.3", "install.sh"))
        self.assertEqual(
            payload["pinned_install_command"],
            "curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.3/install.sh | sudo sh",
        )
        self.assertEqual(set(payload["pinned_asset_urls"]), set(REQUIRED_PUBLIC_ASSETS))
        for name, url in payload["pinned_asset_urls"].items():
            self.assertEqual(url, release_asset_url("v1.2.3", name))
            self.assertNotIn("/releases/latest/", url)

    def test_text_mode_prints_the_concrete_tag(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            latest = write_json(root / "latest.json", base_release_metadata())

            result = subprocess.run(
                ["python3", str(SCRIPT), "--latest-metadata", str(latest)],
                cwd=REPO_ROOT,
                check=True,
                text=True,
                capture_output=True,
            )

        self.assertIn("stable latest resolved: repository=moradology/m80 tag=v1.2.3", result.stdout)
        self.assertIn("install_url=https://github.com/moradology/m80/releases/download/v1.2.3/install.sh", result.stdout)

    def test_library_resolution_returns_the_concrete_tag(self) -> None:
        source = MetadataSource(base_release_metadata(), "fixture:latest.json", "fixture")

        resolution = resolve_latest_bootstrap(source, guard=source, asset_index=base_asset_index())

        self.assertEqual(resolution.resolved_tag, "v1.2.3")
        self.assertEqual(
            resolution.pinned_asset_urls["m80-release-assets.json"],
            release_asset_url("v1.2.3", "m80-release-assets.json"),
        )

    def test_rejects_latest_tag_switch_before_emitting_handoff_json(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            latest = write_json(root / "latest.json", base_release_metadata(tag="v1.2.3"))
            guard = write_json(root / "guard.json", base_release_metadata(tag="v1.2.4"))

            result = subprocess.run(
                [
                    "python3",
                    str(SCRIPT),
                    "--latest-metadata",
                    str(latest),
                    "--guard-metadata",
                    str(guard),
                    "--json",
                ],
                cwd=REPO_ROOT,
                check=False,
                text=True,
                capture_output=True,
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertIn("latest release tag changed during bootstrap resolution", result.stderr)
        self.assertIn("started with v1.2.3, guard observed v1.2.4", result.stderr)
        self.assertIn("releases/download/v1.2.3/install.sh", result.stderr)

    def test_rejects_missing_latest_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            missing = Path(tmp) / "missing.json"

            result = subprocess.run(
                ["python3", str(SCRIPT), "--latest-metadata", str(missing), "--json"],
                cwd=REPO_ROOT,
                check=False,
                text=True,
                capture_output=True,
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertIn("latest release metadata missing", result.stderr)

    def test_rejects_http_failure_before_handoff_json(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            curl = write_fake_curl(root / "curl", exit_code=22, stderr="HTTP 404\n")

            result = subprocess.run(
                [
                    "python3",
                    str(SCRIPT),
                    "--latest-url",
                    "https://api.github.com/repos/moradology/m80/releases/latest",
                    "--curl",
                    str(curl),
                    "--json",
                ],
                cwd=REPO_ROOT,
                check=False,
                text=True,
                capture_output=True,
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertIn("failed to fetch latest release metadata", result.stderr)
        self.assertIn("curl exited 22", result.stderr)

    def test_url_mode_fetches_latest_twice_and_emits_no_mutable_latest_url(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            response = write_json(root / "latest-response.json", base_release_metadata())
            log = root / "curl.log"
            curl = write_fake_curl_json(root / "curl", response=response, log=log)

            result = subprocess.run(
                [
                    "python3",
                    str(SCRIPT),
                    "--latest-url",
                    "https://api.github.com/repos/moradology/m80/releases/latest",
                    "--curl",
                    str(curl),
                    "--json",
                ],
                cwd=REPO_ROOT,
                check=True,
                text=True,
                capture_output=True,
            )
            curl_requests = log.read_text().splitlines()

        payload = json.loads(result.stdout)
        self.assertEqual(payload["resolved_tag"], "v1.2.3")
        self.assertEqual(payload["latest_source_mode"], "url")
        self.assertEqual(payload["tag_switch_guard_source_mode"], "url")
        self.assertEqual(len(curl_requests), 2)
        self.assertNotIn("/releases/latest", result.stdout)

    def test_fixture_mode_reuses_latest_metadata_without_network(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            latest = write_json(root / "latest.json", base_release_metadata())
            curl = write_fake_curl(root / "curl", exit_code=99, stderr="curl should not run\n")

            result = subprocess.run(
                [
                    "python3",
                    str(SCRIPT),
                    "--latest-metadata",
                    str(latest),
                    "--curl",
                    str(curl),
                    "--json",
                ],
                cwd=REPO_ROOT,
                check=True,
                text=True,
                capture_output=True,
            )

        payload = json.loads(result.stdout)
        self.assertEqual(payload["resolved_tag"], "v1.2.3")
        self.assertEqual(payload["latest_source_mode"], "fixture")
        self.assertEqual(payload["tag_switch_guard_source_mode"], "fixture-reused")


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
        "schema_version": 1,
        "release_tag": tag,
        "assets": [
            {
                "name": BUNDLE_NAME,
                "url": release_asset_url(tag, BUNDLE_NAME),
                "sha256": "a" * 64,
                "size_bytes": 42,
                "metadata_name": METADATA_NAME,
                "metadata_sha256": "b" * 64,
                "checksum_name": CHECKSUM_NAME,
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


def write_fake_curl(path: Path, *, exit_code: int, stderr: str) -> Path:
    path.write_text(
        textwrap.dedent(
            f"""\
            #!/usr/bin/env sh
            printf '%s' {json.dumps(stderr)} >&2
            exit {exit_code}
            """
        )
    )
    path.chmod(path.stat().st_mode | stat.S_IXUSR)
    return path


def write_fake_curl_json(path: Path, *, response: Path, log: Path) -> Path:
    path.write_text(
        textwrap.dedent(
            f"""\
            #!/usr/bin/env sh
            printf '%s\\n' "$*" >> {json.dumps(str(log))}
            cat {json.dumps(str(response))}
            """
        )
    )
    path.chmod(path.stat().st_mode | stat.S_IXUSR)
    return path


if __name__ == "__main__":
    unittest.main()
