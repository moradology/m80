#!/usr/bin/env python3
"""Tests for scripts/freshness_drift_evidence.py."""

from __future__ import annotations

import copy
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

from freshness_drift_evidence import build_evidence, render_json, validate_evidence
from release_url_contract import release_asset_url
from stable_release_channel import BUNDLE_NAME, METADATA_NAME


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "freshness_drift_evidence.py"


class FreshnessDriftEvidenceTests(unittest.TestCase):
    def test_success_evidence_is_valid_json(self) -> None:
        evidence = build_evidence(
            stderr="",
            stdout='{"resolved_tag":"v1.2.3"}',
            exit_status=0,
            repository="moradology/m80",
            workflow_run_id="123",
        )

        self.assertEqual(validate_evidence(evidence), [])
        self.assertEqual(evidence["status"], "success")
        self.assertIsNone(evidence["failure_class"])
        self.assertEqual(evidence["resolved_latest_tag"], "v1.2.3")

    def test_docs_drift_records_source_snippet_and_url(self) -> None:
        evidence = failure_evidence(
            "failure_class=docs-drift; freshness public URL fetch failed; "
            "failure=http_failure; "
            "url=https://github.com/moradology/m80/releases/download/v9.9.9/install.sh; "
            "release_tag=v9.9.9; asset=install.sh; role=docs-linked-install; "
            "sources=docs:README.md:72; curl_exit=22; curl exited 22: docs URL is stale"
        )

        self.assert_valid_failure(evidence, "docs-drift")
        self.assertEqual(evidence["source"], {"kind": "file", "value": "README.md", "snippet_id": "README.md:72"})
        self.assertEqual(evidence["expected"]["asset"], "install.sh")
        self.assertEqual(evidence["observed"]["status"], "http_failure")

    def test_missing_asset_records_expected_asset_and_url(self) -> None:
        url = release_asset_url("v1.2.3", METADATA_NAME)

        evidence = failure_evidence(
            "failure_class=missing-public-asset; freshness public asset missing: "
            f"role=metadata asset={METADATA_NAME} url={url} release_tag=v1.2.3"
        )

        self.assert_valid_failure(evidence, "missing-public-asset")
        self.assertEqual(evidence["source"], {"kind": "url", "value": url, "snippet_id": None})
        self.assertEqual(evidence["expected"]["asset"], METADATA_NAME)
        self.assertEqual(evidence["expected"]["value"], url)

    def test_tag_mismatch_records_expected_and_observed_tags(self) -> None:
        evidence = failure_evidence(
            "failure_class=stale-latest; latest release tag changed during bootstrap resolution: "
            "started with v1.2.3, guard observed v1.2.4; failure=latest_tag_switch; "
            "latest_source=latest release metadata from https://api.github.com/repos/moradology/m80/releases/latest; "
            "guard_source=guard latest release metadata from https://api.github.com/repos/moradology/m80/releases/latest"
        )

        self.assert_valid_failure(evidence, "stale-latest")
        self.assertEqual(
            evidence["source"],
            {
                "kind": "url",
                "value": "https://api.github.com/repos/moradology/m80/releases/latest",
                "snippet_id": None,
            },
        )
        self.assertEqual(evidence["expected_stable_tag"], "v1.2.3")
        self.assertEqual(evidence["resolved_latest_tag"], "v1.2.4")
        self.assertEqual(evidence["observed"]["value"], "v1.2.4")

    def test_checksum_mismatch_records_expected_and_observed_digest(self) -> None:
        evidence = failure_evidence(
            "failure_class=checksum-mismatch; freshness public asset digest mismatch: "
            f"role=bundle asset={BUNDLE_NAME} url={release_asset_url('v1.2.3', BUNDLE_NAME)} "
            "release_tag=v1.2.3 expected_sha256=0000000000000000000000000000000000000000000000000000000000000000 "
            "got_sha256=1111111111111111111111111111111111111111111111111111111111111111"
        )

        self.assert_valid_failure(evidence, "checksum-mismatch")
        self.assertEqual(evidence["expected"]["digest"], "0" * 64)
        self.assertEqual(evidence["observed"]["digest"], "1" * 64)

    def test_provenance_mismatch_records_expected_and_observed_value(self) -> None:
        evidence = failure_evidence(
            "failure_class=provenance-mismatch; freshness public asset URL mismatch: "
            f"role=bundle asset={BUNDLE_NAME} expected_url={release_asset_url('v1.2.3', BUNDLE_NAME)} "
            f"got_url={release_asset_url('v1.2.4', BUNDLE_NAME)} release_tag=v1.2.3"
        )

        self.assert_valid_failure(evidence, "provenance-mismatch")
        self.assertEqual(evidence["expected"]["value"], release_asset_url("v1.2.3", BUNDLE_NAME))
        self.assertEqual(evidence["observed"]["value"], release_asset_url("v1.2.4", BUNDLE_NAME))

    def test_transient_network_failure_records_safe_excerpt(self) -> None:
        evidence = failure_evidence(
            "failure_class=network-transient; freshness public URL fetch failed; "
            "failure=timeout; "
            f"url={release_asset_url('v1.2.3', 'install.sh')}; "
            "release_tag=v1.2.3; asset=install.sh; role=pinned-install; "
            "curl_exit=28; curl exited 28: timed out"
        )

        self.assert_valid_failure(evidence, "network-transient")
        self.assertEqual(evidence["source"]["kind"], "url")
        self.assertEqual(evidence["observed"]["status"], "timeout")
        self.assertIn("timed out", evidence["safe_stderr_excerpt"])

    def test_cli_rendering_is_deterministic(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            stderr = root / "stderr.txt"
            stderr.write_text(
                "failure_class=network-transient; failure=timeout; "
                f"url={release_asset_url('v1.2.3', 'install.sh')}; "
                "release_tag=v1.2.3; asset=install.sh; curl_exit=28"
            )
            first = root / "first.json"
            second = root / "second.json"

            for out in [first, second]:
                result = subprocess.run(
                    [
                        "python3",
                        str(SCRIPT),
                        "--stderr-file",
                        str(stderr),
                        "--exit-status",
                        "1",
                        "--workflow-run-id",
                        "123",
                        "--repository",
                        "moradology/m80",
                        "--output",
                        str(out),
                    ],
                    cwd=REPO_ROOT,
                    text=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                )
                self.assertEqual(result.returncode, 0, result.stderr)

            self.assertEqual(first.read_text(), second.read_text())
            self.assertEqual(validate_evidence(json.loads(first.read_text())), [])

    def test_validation_rejects_missing_failure_class(self) -> None:
        evidence = valid_failure()
        del evidence["failure_class"]

        self.assert_has_error(evidence, "missing required field failure_class")

    def test_validation_rejects_missing_source(self) -> None:
        evidence = valid_failure()
        evidence["source"]["value"] = ""

        self.assert_has_error(evidence, "source.value must be a nonempty string")

    def test_validation_rejects_oversized_excerpt(self) -> None:
        evidence = valid_failure()
        evidence["safe_stderr_excerpt"] = "x" * 2001

        self.assert_has_error(evidence, "safe_stderr_excerpt exceeds 2000 bytes")

    def test_validation_rejects_unredacted_secret(self) -> None:
        evidence = valid_failure()
        evidence["safe_stderr_excerpt"] = "leaked ghs_0123456789abcdefghijklmnop"

        self.assert_has_error(evidence, "unredacted secret-looking")

    def test_writer_redacts_github_token_prefixes(self) -> None:
        evidence = failure_evidence(
            "failure_class=network-transient; failure=timeout; "
            "url=https://github.com/moradology/m80/releases/download/v1.2.3/install.sh; "
            "release_tag=v1.2.3; asset=install.sh; "
            "authorization: bearer ghu_0123456789abcdefghijklmnop"
        )

        self.assert_valid_failure(evidence, "network-transient")
        self.assertIn("[REDACTED]", evidence["safe_stderr_excerpt"])
        self.assertNotIn("ghu_0123456789abcdefghijklmnop", evidence["safe_stderr_excerpt"])

    def test_validation_rejects_unknown_schema_version(self) -> None:
        evidence = valid_failure()
        evidence["schema_version"] = 999

        self.assert_has_error(evidence, "schema_version must be 1")

    def test_malformed_docs_source_falls_back_to_url_source(self) -> None:
        evidence = failure_evidence(
            "failure_class=docs-drift; failure=http_failure; "
            "url=https://github.com/moradology/m80/releases/download/v9.9.9/install.sh; "
            "release_tag=v9.9.9; asset=install.sh; sources=docs:README; curl_exit=22"
        )

        self.assert_valid_failure(evidence, "docs-drift")
        self.assertEqual(evidence["source"]["kind"], "url")

    def test_malformed_docs_source_does_not_hide_later_valid_source(self) -> None:
        evidence = failure_evidence(
            "failure_class=docs-drift; failure=http_failure; "
            "url=https://github.com/moradology/m80/releases/download/v9.9.9/install.sh; "
            "release_tag=v9.9.9; asset=install.sh; "
            "sources=docs:README,docs:docs/runbook/release.md:130; curl_exit=22"
        )

        self.assert_valid_failure(evidence, "docs-drift")
        self.assertEqual(
            evidence["source"],
            {
                "kind": "file",
                "value": "docs/runbook/release.md",
                "snippet_id": "docs/runbook/release.md:130",
            },
        )

    def assert_valid_failure(self, evidence: dict, failure_class: str) -> None:
        self.assertEqual(validate_evidence(evidence), [])
        self.assertEqual(evidence["status"], "failure")
        self.assertEqual(evidence["failure_class"], failure_class)
        self.assertEqual(evidence["repository"], "moradology/m80")
        self.assertEqual(evidence["workflow_run_id"], "123")
        self.assertIn("repair_command", evidence)
        self.assertEqual(json.loads(render_json(evidence)), evidence)

    def assert_has_error(self, evidence: dict, expected: str) -> None:
        errors = validate_evidence(evidence)
        self.assertTrue(any(expected in error for error in errors), errors)


def failure_evidence(stderr: str) -> dict:
    return build_evidence(
        stderr=stderr,
        stdout="",
        exit_status=1,
        repository="moradology/m80",
        workflow_run_id="123",
    )


def valid_failure() -> dict:
    return copy.deepcopy(
        failure_evidence(
            "failure_class=network-transient; failure=timeout; "
            f"url={release_asset_url('v1.2.3', 'install.sh')}; "
            "release_tag=v1.2.3; asset=install.sh; curl_exit=28"
        )
    )


if __name__ == "__main__":
    unittest.main()
