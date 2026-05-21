#!/usr/bin/env python3
"""Tests for public release freshness URL bounds."""

from __future__ import annotations

import json
from pathlib import Path
import shlex
import stat
import subprocess
import tempfile
import textwrap
import unittest

from release_url_contract import latest_install_command, release_asset_url
from stable_release_channel import (
    BUNDLE_NAME,
    CHECKSUM_NAME,
    INTEGRITY_ATTESTATION_BUNDLE_NAME,
    METADATA_NAME,
    REQUIRED_PUBLIC_ASSETS,
)


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "release_freshness.py"


class ReleaseFreshnessTest(unittest.TestCase):
    def test_checks_metadata_public_assets_and_docs_urls_with_one_bounded_policy(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            docs_root = write_docs_root(root / "docs-root")
            log = root / "curl.log"
            curl = write_fake_curl(root / "curl", log=log)

            result = subprocess.run(
                [
                    "python3",
                    str(SCRIPT),
                    "--curl",
                    str(curl),
                    "--docs-root",
                    str(docs_root),
                    "--json",
                ],
                cwd=REPO_ROOT,
                check=True,
                text=True,
                capture_output=True,
            )

            payload = json.loads(result.stdout)
            logged = [shlex.split(line) for line in log.read_text().splitlines()]

        self.assertTrue(payload["freshness_network_bounded"])
        self.assertEqual(payload["schema_version"], 1)
        self.assertEqual(payload["resolved_tag"], "v1.2.3")
        self.assertRegex(payload["published_at"], r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$")
        urls = {row["url"] for row in payload["checked_urls"]}
        self.assertIn("https://github.com/moradology/m80/releases/latest/download/install.sh", urls)
        self.assertIn("https://github.com/moradology/m80/releases/download/v1.2.3/install.sh", urls)
        self.assertIn(release_asset_url("v1.2.3", "m80-release-assets.json"), urls)
        self.assertIn(release_asset_url("v1.2.3", INTEGRITY_ATTESTATION_BUNDLE_NAME), urls)
        self.assertIn(release_asset_url("v1.2.3", "m80-release-attestation.json"), urls)
        assets = {row["name"]: row for row in payload["public_assets"]}
        self.assertEqual(set(assets), set(REQUIRED_PUBLIC_ASSETS))
        self.assertEqual(assets["install.sh"]["role"], "installer")
        self.assertEqual(assets["install.sh"]["url"], release_asset_url("v1.2.3", "install.sh"))
        self.assertEqual(assets["install.sh"]["release_tag"], "v1.2.3")
        self.assertEqual(assets[BUNDLE_NAME]["role"], "bundle")
        self.assertEqual(assets[BUNDLE_NAME]["size_bytes"], len(BUNDLE_NAME) * 10)
        self.assertEqual(assets[BUNDLE_NAME]["sha256"], asset_digest(BUNDLE_NAME))
        for name, row in assets.items():
            self.assertEqual(row["url"], release_asset_url("v1.2.3", name))
            self.assertEqual(row["release_tag"], "v1.2.3")
            self.assertIsInstance(row["role"], str)
            self.assertGreater(row["size_bytes"], 0)
            self.assertEqual(row["sha256"], asset_digest(name))

        for args in logged:
            self.assert_curl_flag(args, "--connect-timeout", "10")
            self.assert_curl_flag(args, "--max-time", "120")
            self.assert_curl_flag(args, "--retry", "2")
            self.assert_curl_flag(args, "--retry-delay", "1")
        public_fetches = [args for args in logged if "api.github.com" not in args[-1]]
        self.assertGreaterEqual(len(public_fetches), len(REQUIRED_PUBLIC_ASSETS))
        for args in public_fetches:
            self.assert_curl_flag(args, "--output", "/dev/null")

    def test_failure_classes_name_url_tag_asset_and_source(self) -> None:
        cases = [
            (28, "timeout"),
            (6, "dns_or_connect_failure"),
            (22, "http_failure"),
            (47, "redirect_loop"),
            (18, "partial_download"),
        ]
        for exit_code, failure in cases:
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                docs_root = write_docs_root(root / "docs-root")
                curl = write_fake_curl(
                    root / "curl",
                    log=root / "curl.log",
                    fail_contains="m80-release-assets.json",
                    fail_code=exit_code,
                    fail_stderr=f"{failure}\n",
                )

                result = subprocess.run(
                    [
                        "python3",
                        str(SCRIPT),
                        "--curl",
                        str(curl),
                        "--docs-root",
                        str(docs_root),
                        "--json",
                    ],
                    cwd=REPO_ROOT,
                    check=False,
                    text=True,
                    capture_output=True,
                )

            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout, "")
            self.assertIn(f"failure={failure}", result.stderr)
            self.assertIn(f"curl_exit={exit_code}", result.stderr)
            self.assertIn("release_tag=v1.2.3", result.stderr)
            self.assertIn("asset=m80-release-assets.json", result.stderr)
            self.assertIn("url=https://github.com/moradology/m80/releases/download/v1.2.3/m80-release-assets.json", result.stderr)

    def test_docs_linked_latest_failure_names_docs_source(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            docs_root = write_docs_root(root / "docs-root")
            curl = write_fake_curl(
                root / "curl",
                log=root / "curl.log",
                fail_contains="/releases/latest/download/install.sh",
                fail_code=28,
                fail_stderr="latest install timed out\n",
            )

            result = subprocess.run(
                [
                    "python3",
                    str(SCRIPT),
                    "--curl",
                    str(curl),
                    "--docs-root",
                    str(docs_root),
                    "--json",
                ],
                cwd=REPO_ROOT,
                check=False,
                text=True,
                capture_output=True,
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("failure=timeout", result.stderr)
        self.assertIn("release_tag=latest", result.stderr)
        self.assertIn("asset=install.sh", result.stderr)
        self.assertIn("docs:README.md:", result.stderr)

    def test_missing_public_asset_names_role_url_and_tag(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            docs_root = write_docs_root(root / "docs-root")
            metadata = base_release_metadata()
            metadata["assets"] = [
                asset for asset in metadata["assets"] if asset["name"] != CHECKSUM_NAME
            ]
            curl = write_fake_curl(root / "curl", log=root / "curl.log", metadata=metadata)

            result = subprocess.run(
                [
                    "python3",
                    str(SCRIPT),
                    "--curl",
                    str(curl),
                    "--docs-root",
                    str(docs_root),
                    "--json",
                ],
                cwd=REPO_ROOT,
                check=False,
                text=True,
                capture_output=True,
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("freshness public asset missing", result.stderr)
        self.assertIn(f"role=checksum asset={CHECKSUM_NAME}", result.stderr)
        self.assertIn(f"url={release_asset_url('v1.2.3', CHECKSUM_NAME)}", result.stderr)
        self.assertIn("release_tag=v1.2.3", result.stderr)

    def test_missing_attestation_and_provenance_assets_are_named(self) -> None:
        cases = [
            (INTEGRITY_ATTESTATION_BUNDLE_NAME, "attestation"),
            ("m80-release-integrity.json", "provenance"),
        ]
        for missing, role in cases:
            with self.subTest(missing=missing), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                docs_root = write_docs_root(root / "docs-root")
                metadata = base_release_metadata()
                metadata["assets"] = [
                    asset for asset in metadata["assets"] if asset["name"] != missing
                ]
                curl = write_fake_curl(root / "curl", log=root / "curl.log", metadata=metadata)

                result = subprocess.run(
                    [
                        "python3",
                        str(SCRIPT),
                        "--curl",
                        str(curl),
                        "--docs-root",
                        str(docs_root),
                        "--json",
                    ],
                    cwd=REPO_ROOT,
                    check=False,
                    text=True,
                    capture_output=True,
                )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f"role={role} asset={missing}", result.stderr)
            self.assertIn("release_tag=v1.2.3", result.stderr)

    def test_asset_index_digest_mismatch_fails_before_url_checks(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            docs_root = write_docs_root(root / "docs-root")
            index = write_json(
                root / "assets.json",
                base_asset_index(bundle_sha="0" * 64),
            )
            curl = write_fake_curl(root / "curl", log=root / "curl.log")

            result = subprocess.run(
                [
                    "python3",
                    str(SCRIPT),
                    "--curl",
                    str(curl),
                    "--docs-root",
                    str(docs_root),
                    "--asset-index",
                    str(index),
                    "--json",
                ],
                cwd=REPO_ROOT,
                check=False,
                text=True,
                capture_output=True,
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("freshness public asset digest mismatch", result.stderr)
        self.assertIn(f"role=bundle asset={BUNDLE_NAME}", result.stderr)
        self.assertIn("expected_sha256=" + ("0" * 64), result.stderr)
        self.assertIn("got_sha256=" + asset_digest(BUNDLE_NAME), result.stderr)

    def test_malformed_asset_index_diagnostics_name_index_role(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            docs_root = write_docs_root(root / "docs-root")
            index = write_json(root / "assets.json", {"schema_version": 999, "release_tag": "v1.2.3", "assets": []})
            curl = write_fake_curl(root / "curl", log=root / "curl.log")

            result = subprocess.run(
                [
                    "python3",
                    str(SCRIPT),
                    "--curl",
                    str(curl),
                    "--docs-root",
                    str(docs_root),
                    "--asset-index",
                    str(index),
                    "--json",
                ],
                cwd=REPO_ROOT,
                check=False,
                text=True,
                capture_output=True,
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("freshness asset-index malformed", result.stderr)
        self.assertIn("role=asset-index asset=m80-release-assets.json", result.stderr)
        self.assertIn(f"url={release_asset_url('v1.2.3', 'm80-release-assets.json')}", result.stderr)
        self.assertIn("release_tag=v1.2.3", result.stderr)

    def test_asset_index_required_role_omission_names_asset_url_and_tag(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            docs_root = write_docs_root(root / "docs-root")
            asset_index = base_asset_index()
            asset_index["assets"][0]["attestation_name"] = None
            index = write_json(root / "assets.json", asset_index)
            curl = write_fake_curl(root / "curl", log=root / "curl.log")

            result = subprocess.run(
                [
                    "python3",
                    str(SCRIPT),
                    "--curl",
                    str(curl),
                    "--docs-root",
                    str(docs_root),
                    "--asset-index",
                    str(index),
                    "--json",
                ],
                cwd=REPO_ROOT,
                check=False,
                text=True,
                capture_output=True,
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("freshness asset-index malformed", result.stderr)
        self.assertIn(f"role=attestation asset={INTEGRITY_ATTESTATION_BUNDLE_NAME}", result.stderr)
        self.assertIn(
            f"url={release_asset_url('v1.2.3', INTEGRITY_ATTESTATION_BUNDLE_NAME)}",
            result.stderr,
        )
        self.assertIn("release_tag=v1.2.3", result.stderr)
        self.assertIn("field=attestation_name", result.stderr)

    def assert_curl_flag(self, args: list[str], flag: str, expected_value: str) -> None:
        self.assertIn(flag, args)
        pos = args.index(flag)
        self.assertLess(pos + 1, len(args), args)
        self.assertEqual(args[pos + 1], expected_value, args)


def write_docs_root(root: Path) -> Path:
    root.mkdir()
    (root / "README.md").write_text(
        textwrap.dedent(
            f"""
            # m80 fixture

            Public installer status: m80:public-access-proof m80-o3uh9.21.7 pending.

            ```sh
            {latest_install_command()}
            ```
            """
        ).lstrip()
    )
    return root


def write_fake_curl(
    path: Path,
    *,
    log: Path,
    metadata: dict | None = None,
    fail_contains: str | None = None,
    fail_code: int = 28,
    fail_stderr: str = "failed\n",
) -> Path:
    metadata_json = json.dumps(metadata if metadata is not None else base_release_metadata())
    script = f"""#!/usr/bin/env python3
import shlex
import sys
from pathlib import Path

log = Path({str(log)!r})
log.parent.mkdir(parents=True, exist_ok=True)
with log.open("a") as f:
    f.write(" ".join(shlex.quote(arg) for arg in sys.argv[1:]) + "\\n")

url = sys.argv[-1]
fail_contains = {fail_contains!r}
if fail_contains and fail_contains in url:
    sys.stderr.write({fail_stderr!r})
    raise SystemExit({fail_code})

if "api.github.com" in url:
    sys.stdout.write({metadata_json!r})
"""
    path.write_text(script)
    path.chmod(path.stat().st_mode | stat.S_IXUSR)
    return path


def base_release_metadata(*, tag: str = "v1.2.3") -> dict:
    return {
        "tag_name": tag,
        "draft": False,
        "prerelease": False,
        "assets": [
            {
                "name": name,
                "browser_download_url": release_asset_url(tag, name),
                "digest": "sha256:" + asset_digest(name),
                "size": len(name) * 10,
            }
            for name in REQUIRED_PUBLIC_ASSETS
        ],
    }


def base_asset_index(
    *,
    tag: str = "v1.2.3",
    bundle_sha: str | None = None,
    metadata_sha: str | None = None,
) -> dict:
    return {
        "schema_version": 1,
        "release_tag": tag,
        "assets": [
            {
                "name": BUNDLE_NAME,
                "url": release_asset_url(tag, BUNDLE_NAME),
                "sha256": bundle_sha or asset_digest(BUNDLE_NAME),
                "size_bytes": len(BUNDLE_NAME) * 10,
                "metadata_name": METADATA_NAME,
                "metadata_sha256": metadata_sha or asset_digest(METADATA_NAME),
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


def asset_digest(name: str) -> str:
    return f"{sum(name.encode()):064x}"[-64:]


def write_json(path: Path, value: dict) -> Path:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")
    return path


if __name__ == "__main__":
    unittest.main()
