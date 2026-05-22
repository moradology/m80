#!/usr/bin/env python3
"""Tests for public release freshness URL bounds."""

from __future__ import annotations

import json
import os
from pathlib import Path
import shlex
import stat
import subprocess
import tempfile
import textwrap
import unittest

from freshness_proof import validate_freshness_proof
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
        self.assertEqual(payload["status"], "success")
        self.assertEqual(payload["resolved_tag"], "v1.2.3")
        self.assertEqual(payload["resolved_latest_tag"], "v1.2.3")
        self.assertEqual(payload["workflow_run_id"], os.environ.get("GITHUB_RUN_ID"))
        self.assertRegex(payload["published_at"], r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$")
        self.assertEqual(payload["generated_at"], payload["published_at"])
        self.assertEqual(validate_freshness_proof(payload), [])
        inventory = payload["public_command_inventory"]
        self.assertEqual(inventory["status"], "success")
        self.assertRegex(inventory["digest"], r"^sha256:[0-9a-f]{64}$")
        self.assertEqual(inventory["count"], 1)
        self.assertEqual(inventory["entries"][0]["path"], "README.md")
        self.assertEqual(payload["tag_agreement"]["status"], "success")
        self.assertEqual(payload["tag_agreement"]["latest_tag"], "v1.2.3")
        self.assertEqual(payload["tag_agreement"]["guard_tag"], "v1.2.3")
        self.assertEqual(payload["integrity_result"]["status"], "success")
        self.assertEqual(payload["integrity_result"]["public_asset_count"], len(REQUIRED_PUBLIC_ASSETS))
        self.assertEqual(payload["fixture_install_result"]["status"], "not_run")
        self.assertEqual(payload["failure_taxonomy"]["status"], "success")
        self.assertIn("docs-drift", payload["failure_taxonomy"]["known_classes"])
        self.assertEqual(payload["substrate"]["network_target"], "public-github-release")
        self.assertEqual(payload["substrate"]["auth_state"], "unauthenticated-public-read")
        self.assertFalse(payload["substrate"]["github_write_apis_available"])
        self.assertEqual(
            payload["safety_floor"],
            {
                "schema_version": 1,
                "published_at": payload["published_at"],
                "minimum_safe_tag": None,
                "yanked_releases": [],
            },
        )
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
            self.assertIn("github-release-metadata", row["checksum_sources"])
        self.assertEqual(
            assets["install.sh"]["checksum_sources"],
            ["SHA256SUMS", "github-release-metadata", "install.sh.sha256"],
        )
        self.assertEqual(
            assets[BUNDLE_NAME]["checksum_sources"],
            ["SHA256SUMS", "github-release-metadata", CHECKSUM_NAME],
        )
        self.assertEqual(
            assets[INTEGRITY_ATTESTATION_BUNDLE_NAME]["checksum_sources"],
            ["SHA256SUMS", "github-release-metadata"],
        )

        for args in logged:
            self.assert_curl_flag(args, "--connect-timeout", "10")
            self.assert_curl_flag(args, "--max-time", "120")
            self.assert_curl_flag(args, "--retry", "2")
            self.assert_curl_flag(args, "--retry-delay", "1")
        public_fetches = [args for args in logged if "api.github.com" not in args[-1]]
        self.assertGreaterEqual(len(public_fetches), len(REQUIRED_PUBLIC_ASSETS))
        null_fetches = [args for args in public_fetches if "--output" in args]
        checksum_fetch_urls = {args[-1] for args in public_fetches if "--output" not in args}
        for args in null_fetches:
            self.assert_curl_flag(args, "--output", "/dev/null")
        self.assertGreaterEqual(len(null_fetches), len(REQUIRED_PUBLIC_ASSETS))
        self.assertIn(release_asset_url("v1.2.3", "SHA256SUMS"), checksum_fetch_urls)
        self.assertIn(release_asset_url("v1.2.3", CHECKSUM_NAME), checksum_fetch_urls)

    def test_success_proof_validation_rejects_bad_schema_digest_status_and_order(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            docs_root = write_docs_root(root / "docs-root")
            curl = write_fake_curl(root / "curl", log=root / "curl.log")

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

        missing = clone_json(payload)
        del missing["tag_agreement"]
        self.assertIn("missing required field: tag_agreement", validate_freshness_proof(missing))

        stale_digest = clone_json(payload)
        stale_digest["public_command_inventory"]["digest"] = "sha256:" + ("0" * 64)
        self.assertIn(
            "public_command_inventory.digest does not match entries",
            validate_freshness_proof(stale_digest),
        )

        unknown_status = clone_json(payload)
        unknown_status["fixture_install_result"]["status"] = "maybe"
        self.assertIn(
            "fixture_install_result has unknown status: 'maybe'",
            validate_freshness_proof(unknown_status),
        )

        unsorted_urls = clone_json(payload)
        unsorted_urls["checked_urls"] = list(reversed(unsorted_urls["checked_urls"]))
        self.assertIn("checked_urls must be sorted deterministically", validate_freshness_proof(unsorted_urls))

    def test_failure_classes_name_url_tag_asset_and_source(self) -> None:
        cases = [
            (28, "timeout", "network-transient"),
            (6, "dns_or_connect_failure", "network-transient"),
            (22, "http_failure", "missing-public-asset"),
            (47, "redirect_loop", "network-transient"),
            (18, "partial_download", "network-transient"),
        ]
        for exit_code, failure, failure_class in cases:
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
            self.assertIn(f"failure_class={failure_class}", result.stderr)
            self.assertIn(f"failure={failure}", result.stderr)
            self.assertIn(f"curl_exit={exit_code}", result.stderr)
            self.assertIn("release_tag=v1.2.3", result.stderr)
            self.assertIn("asset=m80-release-assets.json", result.stderr)
            self.assertIn("url=https://github.com/moradology/m80/releases/download/v1.2.3/m80-release-assets.json", result.stderr)
            self.assertIn(f"repair_command={repair_command_for(failure_class)}", result.stderr)

    def test_failure_writes_valid_proof_with_context_and_cli_validation(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            docs_root = write_docs_root(root / "docs-root")
            proof_path = root / "freshness-proof.json"
            curl = write_fake_curl(
                root / "curl",
                log=root / "curl.log",
                fail_contains="m80-release-assets.json",
                fail_code=22,
                fail_stderr="asset missing\n",
            )

            result = subprocess.run(
                [
                    "python3",
                    str(SCRIPT),
                    "--curl",
                    str(curl),
                    "--docs-root",
                    str(docs_root),
                    "--proof-out",
                    str(proof_path),
                    "--json",
                ],
                cwd=REPO_ROOT,
                check=False,
                text=True,
                capture_output=True,
            )

            self.assertNotEqual(result.returncode, 0)
            payload = json.loads(proof_path.read_text())
            self.assertEqual(payload["status"], "failure")
            self.assertEqual(payload["failure"]["class"], "missing-public-asset")
            self.assertEqual(payload["failure"]["repair_command"], repair_command_for("missing-public-asset"))
            self.assertEqual(payload["public_command_inventory"]["status"], "success")
            self.assertEqual(payload["integrity_result"]["status"], "failure")
            self.assertEqual(validate_freshness_proof(payload), [])

            valid = subprocess.run(
                [
                    "python3",
                    str(SCRIPT),
                    "--validate-proof",
                    str(proof_path),
                    "--expected-command-inventory-digest",
                    payload["public_command_inventory"]["digest"],
                ],
                cwd=REPO_ROOT,
                check=True,
                text=True,
                capture_output=True,
            )
            stale = subprocess.run(
                [
                    "python3",
                    str(SCRIPT),
                    "--validate-proof",
                    str(proof_path),
                    "--expected-command-inventory-digest",
                    "sha256:" + ("0" * 64),
                ],
                cwd=REPO_ROOT,
                check=False,
                text=True,
                capture_output=True,
            )

        self.assertIn("freshness proof valid", valid.stdout)
        self.assertNotEqual(stale.returncode, 0)
        self.assertIn("checked_command_inventory_digest does not match proof", stale.stderr)

    def test_metadata_fetch_failure_is_network_transient(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            docs_root = write_docs_root(root / "docs-root")
            curl = write_fake_curl(
                root / "curl",
                log=root / "curl.log",
                fail_contains="api.github.com",
                fail_code=52,
                fail_stderr="empty metadata reply\n",
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
        self.assertIn("failure_class=network-transient", result.stderr)
        self.assertIn("failure=metadata_fetch_failure", result.stderr)
        self.assertIn("fetch_role=initial", result.stderr)
        self.assertIn("repair_command=python3 scripts/release_freshness.py --docs-root . --json", result.stderr)

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
        self.assertIn("failure_class=network-transient", result.stderr)
        self.assertIn("failure=timeout", result.stderr)
        self.assertIn("release_tag=latest", result.stderr)
        self.assertIn("asset=install.sh", result.stderr)
        self.assertIn("docs:README.md:", result.stderr)

    def test_docs_only_pinned_http_failure_is_docs_drift(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            docs_root = write_docs_root(root / "docs-root")
            extra_doc = docs_root / "docs" / "behaviors" / "release" / "docs-only.md"
            extra_doc.parent.mkdir(parents=True)
            extra_doc.write_text(
                "Troubleshooting repair command for a stale install URL:\n\n"
                "```sh\n"
                "curl -fsSL https://github.com/moradology/m80/releases/download/v9.9.9/install.sh | sudo sh\n"
                "```\n"
            )
            curl = write_fake_curl(
                root / "curl",
                log=root / "curl.log",
                fail_contains="/releases/download/v9.9.9/install.sh",
                fail_code=22,
                fail_stderr="docs URL is stale\n",
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
        self.assertIn("failure_class=docs-drift", result.stderr)
        self.assertIn("failure=http_failure", result.stderr)
        self.assertIn("release_tag=v9.9.9", result.stderr)
        self.assertIn("docs:docs/behaviors/release/docs-only.md:", result.stderr)
        self.assertIn("repair_command=python3 scripts/render-freshness-status.py --check", result.stderr)

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
        self.assertIn("failure_class=missing-public-asset", result.stderr)
        self.assertIn("freshness public asset missing", result.stderr)
        self.assertIn(f"role=checksum asset={CHECKSUM_NAME}", result.stderr)
        self.assertIn(f"url={release_asset_url('v1.2.3', CHECKSUM_NAME)}", result.stderr)
        self.assertIn("release_tag=v1.2.3", result.stderr)
        self.assertIn("repair_command=br show m80-o3uh9.21.7", result.stderr)

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
            self.assertIn("failure_class=missing-public-asset", result.stderr)
            self.assertIn(f"role={role} asset={missing}", result.stderr)
            self.assertIn("release_tag=v1.2.3", result.stderr)
            self.assertIn("repair_command=br show m80-o3uh9.21.7", result.stderr)

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
        self.assertIn("failure_class=checksum-mismatch", result.stderr)
        self.assertIn("freshness public asset digest mismatch", result.stderr)
        self.assertIn("role=bundle", result.stderr)
        self.assertIn(f"asset={BUNDLE_NAME}", result.stderr)
        self.assertIn("expected_sha256=" + ("0" * 64), result.stderr)
        self.assertIn("got_sha256=" + asset_digest(BUNDLE_NAME), result.stderr)
        self.assertIn("repair_command=python3 scripts/verify-release-integrity.py --help", result.stderr)

    def test_latest_tag_drift_names_repair_command(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            docs_root = write_docs_root(root / "docs-root")
            latest = write_json(root / "latest.json", base_release_metadata(tag="v1.2.3"))
            guard = write_json(root / "guard.json", base_release_metadata(tag="v1.2.4"))
            curl = write_fake_curl(root / "curl", log=root / "curl.log")

            result = subprocess.run(
                [
                    "python3",
                    str(SCRIPT),
                    "--curl",
                    str(curl),
                    "--docs-root",
                    str(docs_root),
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
        self.assertIn("failure_class=stale-latest", result.stderr)
        self.assertIn("failure=latest_tag_switch", result.stderr)
        self.assertIn("started with v1.2.3, guard observed v1.2.4", result.stderr)
        self.assertIn("repair_command=br show m80-o3uh9.21.8", result.stderr)

    def test_provenance_mismatch_names_repair_command(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            docs_root = write_docs_root(root / "docs-root")
            metadata = base_release_metadata()
            for asset in metadata["assets"]:
                if asset["name"] == BUNDLE_NAME:
                    asset["browser_download_url"] = release_asset_url("v1.2.4", BUNDLE_NAME)
                    break
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
        self.assertIn("failure_class=provenance-mismatch", result.stderr)
        self.assertIn("freshness public asset URL mismatch", result.stderr)
        self.assertIn("expected_url=" + release_asset_url("v1.2.3", BUNDLE_NAME), result.stderr)
        self.assertIn("got_url=" + release_asset_url("v1.2.4", BUNDLE_NAME), result.stderr)
        self.assertIn("repair_command=python3 scripts/verify-release-integrity.py --help", result.stderr)

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
        self.assertIn("failure_class=verifier-schema-drift", result.stderr)
        self.assertIn("freshness asset-index malformed", result.stderr)
        self.assertIn("role=asset-index asset=m80-release-assets.json", result.stderr)
        self.assertIn(f"url={release_asset_url('v1.2.3', 'm80-release-assets.json')}", result.stderr)
        self.assertIn("release_tag=v1.2.3", result.stderr)
        self.assertIn("repair_command=python3 scripts/verify-freshness-failure-policy.py", result.stderr)

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
        self.assertIn("failure_class=verifier-schema-drift", result.stderr)
        self.assertIn("freshness asset-index malformed", result.stderr)
        self.assertIn(f"role=attestation asset={INTEGRITY_ATTESTATION_BUNDLE_NAME}", result.stderr)
        self.assertIn(
            f"url={release_asset_url('v1.2.3', INTEGRITY_ATTESTATION_BUNDLE_NAME)}",
            result.stderr,
        )
        self.assertIn("release_tag=v1.2.3", result.stderr)
        self.assertIn("field=attestation_name", result.stderr)
        self.assertIn("repair_command=python3 scripts/verify-freshness-failure-policy.py", result.stderr)

    def test_checksum_sidecar_digest_mismatch_names_checksum_and_asset(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            docs_root = write_docs_root(root / "docs-root")
            curl = write_fake_curl(
                root / "curl",
                log=root / "curl.log",
                checksum_overrides={CHECKSUM_NAME: f"{'0' * 64}  {BUNDLE_NAME}\n"},
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
        self.assertIn("failure_class=checksum-mismatch", result.stderr)
        self.assertIn("freshness checksum content mismatch: checksum sidecar digest mismatch", result.stderr)
        self.assertIn("role=bundle", result.stderr)
        self.assertIn(f"asset={BUNDLE_NAME}", result.stderr)
        self.assertIn(f"checksum_asset={CHECKSUM_NAME}", result.stderr)
        self.assertIn(f"checksum_url={release_asset_url('v1.2.3', CHECKSUM_NAME)}", result.stderr)
        self.assertIn(f"asset_url={release_asset_url('v1.2.3', BUNDLE_NAME)}", result.stderr)
        self.assertIn("release_tag=v1.2.3", result.stderr)
        self.assertIn("expected=" + asset_digest(BUNDLE_NAME), result.stderr)
        self.assertIn("got=" + ("0" * 64), result.stderr)
        self.assertIn("repair_command=python3 scripts/verify-release-integrity.py --help", result.stderr)

    def test_checksum_sidecar_asset_name_mismatch_names_expected_target(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            docs_root = write_docs_root(root / "docs-root")
            curl = write_fake_curl(
                root / "curl",
                log=root / "curl.log",
                checksum_overrides={CHECKSUM_NAME: f"{asset_digest(BUNDLE_NAME)}  stale-{BUNDLE_NAME}\n"},
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
        self.assertIn("failure_class=checksum-mismatch", result.stderr)
        self.assertIn("freshness checksum content mismatch: checksum sidecar asset-name mismatch", result.stderr)
        self.assertIn(f"asset={BUNDLE_NAME}", result.stderr)
        self.assertIn(f"checksum_asset={CHECKSUM_NAME}", result.stderr)
        self.assertIn(f"expected={BUNDLE_NAME}", result.stderr)
        self.assertIn(f"got=stale-{BUNDLE_NAME}", result.stderr)

    def test_sha256sums_malformed_line_names_checksum_url(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            docs_root = write_docs_root(root / "docs-root")
            curl = write_fake_curl(
                root / "curl",
                log=root / "curl.log",
                checksum_overrides={"SHA256SUMS": "not-a-sha  install.sh\n"},
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
        self.assertIn("failure_class=checksum-mismatch", result.stderr)
        self.assertIn("freshness checksum content mismatch: checksum line malformed", result.stderr)
        self.assertIn("asset=SHA256SUMS", result.stderr)
        self.assertIn("checksum_asset=SHA256SUMS", result.stderr)
        self.assertIn(f"checksum_url={release_asset_url('v1.2.3', 'SHA256SUMS')}", result.stderr)
        self.assertIn("release_tag=v1.2.3", result.stderr)

    def test_sha256sums_duplicate_entry_names_asset(self) -> None:
        duplicate = (
            f"{asset_digest('install.sh')}  install.sh\n"
            f"{asset_digest('install.sh')}  install.sh\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            docs_root = write_docs_root(root / "docs-root")
            curl = write_fake_curl(
                root / "curl",
                log=root / "curl.log",
                checksum_overrides={"SHA256SUMS": duplicate},
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
        self.assertIn("failure_class=checksum-mismatch", result.stderr)
        self.assertIn("freshness checksum content mismatch: duplicate checksum entry", result.stderr)
        self.assertIn("asset=install.sh", result.stderr)
        self.assertIn("checksum_asset=SHA256SUMS", result.stderr)

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


def write_fake_curl(
    path: Path,
    *,
    log: Path,
    metadata: dict | None = None,
    fail_contains: str | None = None,
    fail_code: int = 28,
    fail_stderr: str = "failed\n",
    checksum_overrides: dict[str, str] | None = None,
) -> Path:
    metadata_json = json.dumps(metadata if metadata is not None else base_release_metadata())
    checksums_json = json.dumps(checksum_bodies(checksum_overrides or {}))
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
    raise SystemExit(0)

checksums = {checksums_json}
asset_name = url.rsplit("/", 1)[-1]
if asset_name in checksums:
    sys.stdout.write(checksums[asset_name])
"""
    path.write_text(script)
    path.chmod(path.stat().st_mode | stat.S_IXUSR)
    return path


def checksum_bodies(overrides: dict[str, str]) -> dict[str, str]:
    result = {
        "SHA256SUMS": "".join(
            f"{asset_digest(name)}  {name}\n"
            for name in REQUIRED_PUBLIC_ASSETS
            if name != "SHA256SUMS"
        )
    }
    result.update(
        {
            name: f"{asset_digest(name.removesuffix('.sha256'))}  {name.removesuffix('.sha256')}\n"
            for name in REQUIRED_PUBLIC_ASSETS
            if name.endswith(".sha256")
        }
    )
    result.update(overrides)
    return result


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


def clone_json(value):
    return json.loads(json.dumps(value))


def repair_command_for(failure_class: str) -> str:
    policy = json.loads(
        (REPO_ROOT / "docs" / "behaviors" / "release" / "freshness-failure-policy.json").read_text()
    )
    for row in policy["failure_classes"]:
        if row["id"] == failure_class:
            return row["repair_command"]
    raise AssertionError(f"missing freshness failure policy row: {failure_class}")


if __name__ == "__main__":
    unittest.main()
