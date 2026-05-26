#!/usr/bin/env python3
"""Tests for scripts/lint-github-workflows.py."""

from __future__ import annotations

import json
import os
import re
import subprocess
from pathlib import Path
import tempfile
import textwrap
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
LINT = REPO_ROOT / "scripts" / "lint-github-workflows.py"
AUTHORITY = REPO_ROOT / "scripts" / "release_publish_authority.py"


class WorkflowPolicyTest(unittest.TestCase):
    def test_repo_workflows_pass_policy(self) -> None:
        result = run_lint(REPO_ROOT / ".github" / "workflows")

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_renamed_release_workflow_uses_configured_scope(self) -> None:
        with workflow_dir(
            "ship.yml",
            """
            name: Ship artifacts
            on:
              push:
                tags: ["v*"]
            permissions:
              contents: read
            jobs:
              build:
                permissions:
                  contents: read
                runs-on: ubuntu-latest
                timeout-minutes: 30
                steps:
                  - uses: actions/checkout@v6
            """,
            scope="release-authority",
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("release workflow must declare top-level concurrency.group", result.stderr)

    def test_guarded_looking_filename_without_config_is_rejected(self) -> None:
        with workflow_dir(
            "release-ish.yml",
            """
            name: Release-ish
            on: [push]
            permissions:
              contents: read
            jobs:
              test:
                runs-on: ubuntu-latest
                steps:
                  - uses: actions/checkout@v6
            """,
            policy_entries=[],
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("guarded-looking workflow filename is not named", result.stderr)

    def test_ordinary_ci_scope_does_not_require_release_guards(self) -> None:
        with workflow_dir(
            "ci.yml",
            """
            name: CI
            on: [push]
            permissions:
              contents: read
            jobs:
              test:
                runs-on: ubuntu-latest
                steps:
                  - uses: actions/checkout@v6
            """,
            scope="ordinary-ci",
        ) as root:
            result = run_lint(root)

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_ci_requires_changed_line_whitespace_check(self) -> None:
        with workflow_dir("ci.yml", ci_workflow(diff_check_step="")) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("CI workflow must run git diff --check before Rust steps", result.stderr)

    def test_ci_diff_check_requires_push_and_pr_base_revisions(self) -> None:
        with workflow_dir(
            "ci.yml",
            ci_workflow(
                diff_check_step="""
                - name: changed-line whitespace check
                  run: |
                    set -euo pipefail
                    git diff --check HEAD~1...HEAD
                """
            ),
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("must use github.event.before for push", result.stderr)
        self.assertIn("must use pull_request.base.sha for PRs", result.stderr)
        self.assertIn("must prove the base commit exists", result.stderr)
        self.assertIn("must handle branch-creation push events", result.stderr)

    def test_ci_diff_check_must_precede_expensive_rust_steps(self) -> None:
        workflow = ci_workflow(diff_check_step="")
        workflow = workflow.replace(
            "      - name: cargo build\n        run: cargo build --workspace\n",
            "      - name: cargo build\n        run: cargo build --workspace\n"
            + textwrap.indent(textwrap.dedent(ci_diff_check_step()).strip(), "      ")
            + "\n",
        )
        with workflow_dir("ci.yml", workflow) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("git diff --check must run before expensive Rust steps", result.stderr)

    def test_ci_accepts_changed_line_whitespace_check_before_rust(self) -> None:
        with workflow_dir("ci.yml", ci_workflow()) as root:
            result = run_lint(root)

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_privileged_e2e_requires_ephemeral_runner_label_contract(self) -> None:
        with workflow_dir("e2e-privileged.yml", privileged_e2e_workflow()) as root:
            result = run_lint(root)

        self.assertEqual(result.returncode, 0, result.stderr)

        with workflow_dir(
            "e2e-privileged.yml",
            privileged_e2e_workflow().replace("runner_label:", "runner_slot:"),
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("must expose runner_label input", result.stderr)

    def test_privileged_e2e_requires_target_sha_checkout_contract(self) -> None:
        workflow = privileged_e2e_workflow()
        cases = [
            ("target_sha:", "must expose target_sha input"),
            ("M80_E2E_TARGET_SHA: ${{ inputs.target_sha || '' }}", "must pass through target_sha input"),
            ('[[ "$M80_E2E_TARGET_SHA" =~ ^[0-9a-fA-F]{40}$ ]]', "must validate target_sha"),
            ("refs/pull/$M80_E2E_PULL_NUMBER/head", "must fetch PR head"),
            ('git -C "$GITHUB_WORKSPACE" checkout --force "$target_sha"', "must check out the requested target_sha"),
            ('checked_out_sha="$(git -C "$GITHUB_WORKSPACE" rev-parse HEAD)"', "must verify the checked-out commit"),
            ('checked_out_sha="$(git rev-parse HEAD)"', "must capture the checked-out SHA in diagnostics"),
            ("printf 'M80_E2E_CHECKED_OUT_SHA=%s\\n' \"$checked_out_sha\"", "must record the checked-out SHA"),
        ]
        for token, expected_error in cases:
            with self.subTest(token=token):
                with workflow_dir("e2e-privileged.yml", workflow.replace(token, "")) as root:
                    result = run_lint(root)

                self.assertNotEqual(result.returncode, 0)
                self.assertIn(expected_error, result.stderr)

    def test_workflow_scope_policy_rejects_duplicate_entries(self) -> None:
        with workflow_dir(
            "ci.yml",
            """
            name: CI
            on: [push]
            permissions:
              contents: read
            jobs:
              test:
                runs-on: ubuntu-latest
                steps:
                  - uses: actions/checkout@v6
            """,
            policy_entries=[
                {"path": "ci.yml", "scope": "ordinary-ci"},
                {"path": "ci.yml", "scope": "ordinary-ci"},
            ],
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("duplicate workflow scope entry for ci.yml", result.stderr)

    def test_workflow_scope_policy_rejects_unknown_scope(self) -> None:
        with workflow_dir(
            "ci.yml",
            """
            name: CI
            on: [push]
            permissions:
              contents: read
            jobs:
              test:
                runs-on: ubuntu-latest
                steps:
                  - uses: actions/checkout@v6
            """,
            scope="nightly-magic",
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unknown workflow scope: nightly-magic", result.stderr)

    def test_workflow_scope_policy_rejects_configured_missing_file(self) -> None:
        with workflow_dir(
            "ci.yml",
            """
            name: CI
            on: [push]
            permissions:
              contents: read
            jobs:
              test:
                runs-on: ubuntu-latest
                steps:
                  - uses: actions/checkout@v6
            """,
            policy_entries=[
                {"path": "ci.yml", "scope": "ordinary-ci"},
                {"path": "release-artifacts.yml", "scope": "release-authority"},
            ],
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("configured workflow is missing: release-artifacts.yml", result.stderr)

    def test_release_workflow_requires_concurrency(self) -> None:
        with workflow_dir(
            "release-artifacts.yml",
            """
            name: Release artifacts
            on:
              push:
                tags: ["v*"]
            permissions:
              contents: read
            jobs:
              build:
                permissions:
                  contents: read
                runs-on: ubuntu-latest
                steps:
                  - uses: actions/checkout@v4
            """,
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("release workflow must declare top-level concurrency.group", result.stderr)

    def test_latest_workflow_uses_release_guards(self) -> None:
        with workflow_dir(
            "latest-freshness.yml",
            """
            name: Latest freshness
            on:
              schedule:
                - cron: "17 4 * * *"
            permissions:
              contents: read
            jobs:
              verify:
                permissions:
                  contents: read
                runs-on: ubuntu-latest
                steps:
                  - uses: actions/checkout@v4
            """,
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("release workflow must declare top-level concurrency.group", result.stderr)

    def test_freshness_workflow_clean_scheduled_manual_artifact_shape_is_allowed(self) -> None:
        with workflow_dir("latest-freshness.yml", latest_freshness_workflow()) as root:
            result = run_lint(root)

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_freshness_workflow_requires_schedule_and_manual_dispatch(self) -> None:
        with workflow_dir(
            "latest-freshness.yml",
            latest_freshness_workflow(
                events="""
                workflow_dispatch:
                """
            ),
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("freshness workflow must declare a schedule trigger", result.stderr)

        with workflow_dir(
            "latest-freshness.yml",
            latest_freshness_workflow(
                events="""
                schedule:
                  - cron: "17 4 * * *"
                """
            ),
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("freshness workflow must declare workflow_dispatch", result.stderr)

    def test_freshness_workflow_requires_read_only_permissions(self) -> None:
        with workflow_dir(
            "latest-freshness.yml",
            latest_freshness_workflow(job_permissions="contents: write"),
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("job hostless-public-freshness must not grant contents: write", result.stderr)

    def test_freshness_workflow_keeps_evidence_runs_uncanceled(self) -> None:
        with workflow_dir(
            "latest-freshness.yml",
            latest_freshness_workflow(cancel_in_progress="true"),
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("freshness workflow concurrency must set cancel-in-progress: false", result.stderr)

    def test_freshness_workflow_requires_always_artifact_upload(self) -> None:
        with workflow_dir(
            "latest-freshness.yml",
            latest_freshness_workflow(upload_artifact=False),
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("freshness workflow must upload proof/log artifacts with if: always()", result.stderr)

    def test_freshness_workflow_requires_stderr_artifact_upload(self) -> None:
        with workflow_dir(
            "latest-freshness.yml",
            latest_freshness_workflow(upload_stderr_artifact=False),
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("freshness workflow must upload proof/log artifacts with if: always()", result.stderr)

    def test_freshness_workflow_requires_drift_artifact_upload(self) -> None:
        with workflow_dir(
            "latest-freshness.yml",
            latest_freshness_workflow(upload_drift_artifact=False),
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("freshness workflow must upload proof/log artifacts with if: always()", result.stderr)

    def test_freshness_workflow_requires_public_latest_proof_publish(self) -> None:
        with workflow_dir(
            "latest-freshness.yml",
            latest_freshness_workflow(publish_latest_proof=False),
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("must publish m80-latest-freshness-proof.json", result.stderr)

    def test_freshness_workflow_rejects_mutation_commands_and_privileged_runner(self) -> None:
        with workflow_dir(
            "latest-freshness.yml",
            latest_freshness_workflow(command="gh release upload v0.1.0 /tmp/asset"),
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("freshness workflow must not run release mutation command", result.stderr)

        with workflow_dir(
            "latest-freshness.yml",
            latest_freshness_workflow(runs_on="[self-hosted, real-kvm]"),
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("freshness job hostless-public-freshness must stay hostless", result.stderr)

    def test_freshness_publish_write_requires_readiness_needs(self) -> None:
        workflow = latest_freshness_workflow().replace("  publish-latest-freshness:\n    needs: hostless-public-freshness\n", "  publish-latest-freshness:\n")
        with workflow_dir("latest-freshness.yml", workflow) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("job publish-latest-freshness grants contents: write", result.stderr)
        self.assertIn("requires freshness readiness needs", result.stderr)

    def test_release_write_token_is_publish_only(self) -> None:
        with workflow_dir(
            "release-artifacts.yml",
            """
            name: Release artifacts
            on:
              push:
                tags: ["v*"]
            permissions:
              contents: read
            concurrency:
              group: release-${{ github.ref_name }}
            jobs:
              build:
                permissions:
                  contents: write
                runs-on: ubuntu-latest
                steps:
                  - uses: actions/checkout@v4
            """,
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("job build must not grant contents: write", result.stderr)

    def test_release_workflow_rejects_workflow_level_write_permission(self) -> None:
        with workflow_dir(
            "release-artifacts.yml",
            release_artifact_origin_workflow().replace(
                "permissions:\n  contents: read",
                "permissions:\n  contents: write",
            ),
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("top-level permissions must not grant contents: write", result.stderr)

    def test_release_publish_write_requires_build_needs(self) -> None:
        workflow = release_artifact_origin_workflow().replace("    needs: build-release-artifacts\n", "")
        with workflow_dir("release-artifacts.yml", workflow) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("job publish-release-artifacts grants contents: write", result.stderr)
        self.assertIn("requires needs: build-release-artifacts", result.stderr)

    def test_release_publish_requires_protected_environment(self) -> None:
        workflow = release_artifact_origin_workflow(publish_environment=None)
        with workflow_dir("release-artifacts.yml", workflow) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("job publish-release-artifacts must target protected environment", result.stderr)
        self.assertIn("m80-release-publish", result.stderr)

    def test_release_protected_environment_is_publish_only(self) -> None:
        workflow = release_artifact_origin_workflow(
            extra_build_job_lines="environment: m80-release-publish",
        )
        with workflow_dir("release-artifacts.yml", workflow) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("job build-release-artifacts must not target protected environment", result.stderr)

    def test_release_mutation_commands_are_publish_only(self) -> None:
        workflow = release_artifact_origin_workflow(
            extra_build_steps="""
            - name: Bad release mutation
              run: |
                set -euo pipefail
                gh release upload "$GITHUB_REF_NAME" /tmp/asset
            """,
        )
        with workflow_dir("release-artifacts.yml", workflow) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("job build-release-artifacts must not run release mutation command", result.stderr)

    def test_release_build_attestation_permissions_are_allowed(self) -> None:
        with workflow_dir(
            "release-artifacts.yml",
            release_artifact_origin_workflow(
                extra_build_steps="""
                - uses: actions/attest@v4
                  with:
                    subject-name: m80-release-integrity.json
                    subject-digest: sha256:0123456789abcdef
                """
            ),
        ) as root:
            result = run_lint(root)

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_release_artifact_origin_handoff_is_allowed(self) -> None:
        with workflow_dir("release-artifacts.yml", release_artifact_origin_workflow()) as root:
            result = run_lint(root)

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_release_artifact_origin_rejects_manual_override_inputs(self) -> None:
        workflow = release_artifact_origin_workflow(
            events="""
            push:
              tags: ["v*"]
            workflow_dispatch:
              inputs:
                artifact_url:
                  required: true
            """
        )
        with workflow_dir("release-artifacts.yml", workflow) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("release workflow must not accept manual artifact override inputs", result.stderr)

    def test_release_artifact_origin_rejects_cache_path_injection(self) -> None:
        workflow = release_artifact_origin_workflow(
            extra_build_steps="""
            - uses: actions/cache@v4
              with:
                path: /tmp/m80-release-dist
                key: release-dist
            """
        )
        with workflow_dir("release-artifacts.yml", workflow) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("release workflow must not use cache contents as release artifact input", result.stderr)

    def test_release_artifact_origin_rejects_runner_local_dist_reuse(self) -> None:
        workflow = release_artifact_origin_workflow().replace(
            "path: ${{ steps.publish-scratch.outputs.upload_dir }}",
            "path: ${{ steps.build-scratch.outputs.dist_dir }}",
        )
        with workflow_dir("release-artifacts.yml", workflow) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("publish job must not reuse runner-local dist paths", result.stderr)

    def test_release_artifact_origin_rejects_wrong_artifact_id_handoff(self) -> None:
        workflow = release_artifact_origin_workflow().replace(
            "artifact-ids: ${{ needs.build-release-artifacts.outputs.release_dist_artifact_id }}",
            "name: ${{ needs.build-release-artifacts.outputs.release_dist_artifact_name }}",
        )
        with workflow_dir("release-artifacts.yml", workflow) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("publish download must use the recorded build artifact id", result.stderr)

    def test_release_artifact_origin_rejects_nested_download_layout(self) -> None:
        workflow = release_artifact_origin_workflow().replace(
            "          merge-multiple: true\n",
            "",
        )
        with workflow_dir("release-artifacts.yml", workflow) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            "publish download must flatten the selected artifact into the verified upload directory",
            result.stderr,
        )

    def test_release_temp_isolation_rejects_fixed_tmp_literals(self) -> None:
        workflow = release_artifact_origin_workflow().replace(
            "$RUNNER_TEMP/m80-release-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-build",
            "/tmp/m80-release-dist",
        )
        with workflow_dir("release-artifacts.yml", workflow) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("release workflow must not use fixed /tmp/m80-release-* scratch paths", result.stderr)

    def test_release_temp_isolation_rejects_preexisting_path_reuse(self) -> None:
        workflow = release_artifact_origin_workflow().replace('          test ! -e "$release_tmp_root"\n', "")
        with workflow_dir("release-artifacts.yml", workflow) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("must isolate and validate runner temp scratch root", result.stderr)

    def test_release_temp_isolation_rejects_symlink_staging_root(self) -> None:
        workflow = release_artifact_origin_workflow().replace('          test ! -L "$release_tmp_root"\n', "")
        with workflow_dir("release-artifacts.yml", workflow) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("must isolate and validate runner temp scratch root", result.stderr)

    def test_release_temp_isolation_rejects_unsafe_mode_or_owner(self) -> None:
        workflow = release_artifact_origin_workflow().replace('          test "$mode_octal" = "700"\n', "")
        with workflow_dir("release-artifacts.yml", workflow) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("must isolate and validate runner temp scratch root", result.stderr)

    def test_publish_authority_policy_allows_clean_publish_context(self) -> None:
        with workflow_dir("release-artifacts.yml", publish_authority_workflow()) as root:
            receipt = root / "token-authority.json"
            result = run_authority(root / "release-artifacts.yml", receipt=receipt, write=True)
            payload = json.loads(receipt.read_text())
            receipt_text = receipt.read_text()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(payload["kind"], "m80_release_publish_token_authority")
        self.assertEqual(payload["decision"], "approved")
        self.assertEqual(payload["repository"], "moradology/m80")
        self.assertEqual(payload["release_tag"], "v0.1.0")
        self.assertEqual(payload["token_source"], "github.token")
        self.assertEqual(payload["probes"][0]["name"], "release_metadata")
        self.assertNotIn("ghs_test_token", receipt_text)
        self.assertNotIn("authorization", receipt_text.lower())

    def test_publish_authority_policy_rejects_wrong_repository(self) -> None:
        with workflow_dir("release-artifacts.yml", publish_authority_workflow()) as root:
            receipt = root / "token-authority.json"
            result = run_authority(
                root / "release-artifacts.yml",
                receipt=receipt,
                write=True,
                repository="example/m80",
                workflow_ref="example/m80/.github/workflows/release-artifacts.yml@refs/tags/v0.1.0",
            )
            payload = json.loads(receipt.read_text())

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("repository mismatch", result.stderr)
        self.assertEqual(payload["decision"], "failed")

    def test_publish_authority_policy_rejects_branch_ref(self) -> None:
        with workflow_dir("release-artifacts.yml", publish_authority_workflow()) as root:
            result = run_authority(
                root / "release-artifacts.yml",
                github_ref="refs/heads/main",
                workflow_ref="moradology/m80/.github/workflows/release-artifacts.yml@refs/heads/main",
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("github-ref must match refs/tags/v*", result.stderr)

    def test_publish_authority_policy_rejects_wrong_job(self) -> None:
        with workflow_dir("release-artifacts.yml", publish_authority_workflow()) as root:
            result = run_authority(root / "release-artifacts.yml", github_job="build-release-artifacts")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("job mismatch", result.stderr)

    def test_publish_authority_policy_rejects_wrong_workflow_ref(self) -> None:
        with workflow_dir("release-artifacts.yml", publish_authority_workflow()) as root:
            result = run_authority(
                root / "release-artifacts.yml",
                workflow_ref="moradology/m80/.github/workflows/not-release.yml@refs/tags/v0.1.0",
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("workflow-ref mismatch", result.stderr)

    def test_publish_authority_policy_rejects_missing_token_source(self) -> None:
        with workflow_dir("release-artifacts.yml", publish_authority_workflow()) as root:
            result = run_authority(root / "release-artifacts.yml", token_source=None)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("token-source missing", result.stderr)

    def test_publish_authority_policy_rejects_wrong_token_source(self) -> None:
        with workflow_dir("release-artifacts.yml", publish_authority_workflow()) as root:
            result = run_authority(root / "release-artifacts.yml", token_source="secrets.RELEASE_TOKEN")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("token-source mismatch", result.stderr)

    def test_publish_authority_policy_rejects_missing_token_material(self) -> None:
        with workflow_dir("release-artifacts.yml", publish_authority_workflow()) as root:
            receipt = root / "token-authority.json"
            result = run_authority(root / "release-artifacts.yml", receipt=receipt, write=True, token_present=False)
            payload = json.loads(receipt.read_text())

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("token env GH_TOKEN missing", result.stderr)
        self.assertIn("token env GH_TOKEN missing", payload["failure_reason"])

    def test_publish_authority_policy_accepts_absent_release_metadata(self) -> None:
        with workflow_dir("release-artifacts.yml", publish_authority_workflow()) as root:
            receipt = root / "token-authority.json"
            result = run_authority(root / "release-artifacts.yml", receipt=receipt, write=True, gh_mode="absent")
            payload = json.loads(receipt.read_text())

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(payload["decision"], "approved")
        self.assertEqual(payload["probes"][0]["release_state"], "absent")
        self.assertEqual(payload["probes"][0]["exit_status"], 1)
        self.assertIn("release not found", payload["probes"][0]["stderr"])

    def test_publish_authority_policy_rejects_unreadable_release_metadata(self) -> None:
        with workflow_dir("release-artifacts.yml", publish_authority_workflow()) as root:
            receipt = root / "token-authority.json"
            result = run_authority(root / "release-artifacts.yml", receipt=receipt, write=True, gh_mode="read_only")
            payload = json.loads(receipt.read_text())
            receipt_text = receipt.read_text()

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("release metadata unreadable", result.stderr)
        self.assertEqual(payload["decision"], "failed")
        self.assertIn("release metadata unreadable", payload["failure_reason"])
        self.assertEqual(payload["probes"][0]["exit_status"], 1)
        self.assertIn("resource not accessible", payload["probes"][0]["stderr"])
        self.assertNotIn("ghs_test_token", receipt_text)

    def test_publish_authority_policy_rejects_unavailable_release_api(self) -> None:
        with workflow_dir("release-artifacts.yml", publish_authority_workflow()) as root:
            receipt = root / "token-authority.json"
            result = run_authority(root / "release-artifacts.yml", receipt=receipt, write=True, gh_mode="unavailable")
            payload = json.loads(receipt.read_text())

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("release metadata unreadable", result.stderr)
        self.assertEqual(payload["probes"][0]["exit_status"], 2)
        self.assertIn("GitHub API unavailable", payload["probes"][0]["stderr"])

    def test_publish_authority_policy_rejects_malformed_receipt(self) -> None:
        with workflow_dir("release-artifacts.yml", publish_authority_workflow()) as root:
            receipt = root / "token-authority.json"
            receipt.write_text('{"kind":"wrong"}\n')
            result = run_authority(root / "release-artifacts.yml", receipt=receipt)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("receipt missing fields", result.stderr)

    def test_publish_authority_policy_rejects_unexpected_write_authority(self) -> None:
        workflow = publish_authority_workflow().replace(
            "          contents: read\n          id-token: write",
            "          contents: write\n          id-token: write",
        )
        with workflow_dir("release-artifacts.yml", workflow) as root:
            result = run_authority(root / "release-artifacts.yml")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unexpected write authority", result.stderr)

    def test_release_job_requires_timeout_minutes(self) -> None:
        with workflow_dir(
            "release-artifacts.yml",
            """
            name: Release artifacts
            on:
              push:
                tags: ["v*"]
            permissions:
              contents: read
            concurrency:
              group: release-${{ github.ref_name }}
            jobs:
              build:
                permissions:
                  contents: read
                runs-on: ubuntu-latest
                steps:
                  - uses: actions/checkout@v4
            """,
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("release job build must declare timeout-minutes", result.stderr)

    def test_release_job_timeout_has_upper_bound(self) -> None:
        with workflow_dir(
            "release-artifacts.yml",
            """
            name: Release artifacts
            on:
              push:
                tags: ["v*"]
            permissions:
              contents: read
            concurrency:
              group: release-${{ github.ref_name }}
            jobs:
              build:
                permissions:
                  contents: read
                runs-on: ubuntu-latest
                timeout-minutes: 999
                steps:
                  - uses: actions/checkout@v4
            """,
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("release job build timeout-minutes 999 exceeds maximum 120", result.stderr)

    def test_release_job_timeout_within_bound_is_allowed(self) -> None:
        with workflow_dir(
            "release-artifacts.yml",
            """
            name: Release artifacts
            on:
              push:
                tags: ["v*"]
            permissions:
              contents: read
            concurrency:
              group: release-${{ github.ref_name }}
            jobs:
              build:
                permissions:
                  contents: read
                runs-on: ubuntu-latest
                timeout-minutes: 30
                steps:
                  - uses: actions/checkout@v4
            """,
        ) as root:
            result = run_lint(root)

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_release_job_timeout_must_match_configured_budget(self) -> None:
        with workflow_dir(
            "release-artifacts.yml",
            """
            name: Release artifacts
            on:
              push:
                tags: ["v*"]
            permissions:
              contents: read
            concurrency:
              group: release-${{ github.ref_name }}
            jobs:
              build:
                permissions:
                  contents: read
                runs-on: ubuntu-latest
                timeout-minutes: 30
                steps:
                  - uses: actions/checkout@v6
            """,
            timeout_entries=[
                {"path": "release-artifacts.yml", "job": "build", "timeout_minutes": 31}
            ],
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("timeout-minutes 30 does not match configured budget 31", result.stderr)

    def test_release_job_requires_timeout_budget_config_entry(self) -> None:
        with workflow_dir(
            "release-artifacts.yml",
            """
            name: Release artifacts
            on:
              push:
                tags: ["v*"]
            permissions:
              contents: read
            concurrency:
              group: release-${{ github.ref_name }}
            jobs:
              build:
                permissions:
                  contents: read
                runs-on: ubuntu-latest
                timeout-minutes: 30
                steps:
                  - uses: actions/checkout@v6
            """,
            timeout_entries=[],
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("release job build missing timeout budget config entry", result.stderr)

    def test_reusable_release_job_timeout_marker_is_allowed(self) -> None:
        with workflow_dir(
            "proof.yml",
            """
            name: Release proof
            on: workflow_call
            permissions:
              contents: read
            concurrency:
              group: proof-${{ github.ref_name }}
            jobs:
              real-kvm-proof:
                # m80-lint: reusable-timeout-minutes=45
                uses: ./.github/workflows/reusable-proof.yml
                permissions:
                  contents: read
            """,
        ) as root:
            result = run_lint(root)

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_reusable_release_job_timeout_marker_must_match_configured_budget(self) -> None:
        with workflow_dir(
            "proof.yml",
            """
            name: Release proof
            on: workflow_call
            permissions:
              contents: read
            concurrency:
              group: proof-${{ github.ref_name }}
            jobs:
              real-kvm-proof:
                # m80-lint: reusable-timeout-minutes=45
                uses: ./.github/workflows/reusable-proof.yml
                permissions:
                  contents: read
            """,
            timeout_entries=[
                {"path": "proof.yml", "job": "real-kvm-proof", "timeout_minutes": 46}
            ],
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("timeout marker 45 does not match configured budget 46", result.stderr)

    def test_reusable_release_job_requires_documented_timeout(self) -> None:
        with workflow_dir(
            "proof.yml",
            """
            name: Release proof
            on: workflow_call
            permissions:
              contents: read
            concurrency:
              group: proof-${{ github.ref_name }}
            jobs:
              real-kvm-proof:
                uses: ./.github/workflows/reusable-proof.yml
                permissions:
                  contents: read
            """,
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("release reusable job real-kvm-proof must declare timeout-minutes", result.stderr)

    def test_timeout_budget_runbook_table_must_match_config(self) -> None:
        with workflow_dir(
            "release-artifacts.yml",
            """
            name: Release artifacts
            on:
              push:
                tags: ["v*"]
            permissions:
              contents: read
            concurrency:
              group: release-${{ github.ref_name }}
            jobs:
              build:
                permissions:
                  contents: read
                runs-on: ubuntu-latest
                timeout-minutes: 30
                steps:
                  - uses: actions/checkout@v6
            """,
            runbook_text="""
            | Workflow | Job | Timeout |
            | --- | --- | --- |
            | `.github/workflows/release-artifacts.yml` | `build` | 31 minutes |
            """,
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("workflow timeout budget table is stale", result.stderr)
        self.assertIn("| `.github/workflows/release-artifacts.yml` | `build` | 30 minutes |", result.stderr)

    def test_release_attestation_requires_oidc_and_attestation_permissions(self) -> None:
        with workflow_dir(
            "release-artifacts.yml",
            """
            name: Release artifacts
            on:
              push:
                tags: ["v*"]
            permissions:
              contents: read
            concurrency:
              group: release-${{ github.ref_name }}
            jobs:
              build-release-artifacts:
                permissions:
                  contents: read
                runs-on: ubuntu-latest
                steps:
                  - uses: actions/attest@v4
                    with:
                      subject-name: m80-release-integrity.json
                      subject-digest: sha256:0123456789abcdef
            """,
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("must grant id-token: write", result.stderr)
        self.assertIn("must grant attestations: write", result.stderr)

    def test_non_build_job_cannot_generate_release_attestations(self) -> None:
        with workflow_dir(
            "release-artifacts.yml",
            """
            name: Release artifacts
            on:
              push:
                tags: ["v*"]
            permissions:
              contents: read
            concurrency:
              group: release-${{ github.ref_name }}
            jobs:
              publish-release-artifacts:
                permissions:
                  contents: write
                  id-token: write
                  attestations: write
                runs-on: ubuntu-latest
                steps:
                  - uses: actions/attest@v4
                    with:
                      subject-name: m80-release-integrity.json
                      subject-digest: sha256:0123456789abcdef
            """,
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("must not grant id-token: write", result.stderr)
        self.assertIn("must not generate release attestations", result.stderr)

    def test_job_write_all_permission_is_rejected(self) -> None:
        with workflow_dir(
            "release-artifacts.yml",
            """
            name: Release artifacts
            on:
              push:
                tags: ["v*"]
            permissions:
              contents: read
            concurrency:
              group: release-${{ github.ref_name }}
            jobs:
              build:
                permissions: write-all
                runs-on: ubuntu-latest
                steps:
                  - uses: actions/checkout@v4
            """,
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("job build must not grant *: write-all", result.stderr)

    def test_release_cargo_invocation_requires_locked(self) -> None:
        with workflow_dir(
            "release-artifacts.yml",
            """
            name: Release artifacts
            on:
              push:
                tags: ["v*"]
            permissions:
              contents: read
            concurrency:
              group: release-${{ github.ref_name }}
            jobs:
              build:
                permissions:
                  contents: read
                runs-on: ubuntu-latest
                steps:
                  - uses: actions/checkout@v4
                  - run: cargo build -p m80-cli --release
            """,
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("release cargo invocation must use --locked", result.stderr)

    def test_release_rust_toolchain_install_requires_numeric_pin(self) -> None:
        with workflow_dir(
            "release-artifacts.yml",
            """
            name: Release artifacts
            on:
              push:
                tags: ["v*"]
            permissions:
              contents: read
            concurrency:
              group: release-${{ github.ref_name }}
            jobs:
              build:
                permissions:
                  contents: read
                runs-on: ubuntu-latest
                steps:
                  - uses: actions/checkout@v4
                  - run: rustup toolchain install stable --profile minimal
            """,
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("release rustup toolchain install must use a pinned numeric toolchain", result.stderr)

    def test_release_target_add_requires_pinned_toolchain(self) -> None:
        with workflow_dir(
            "release-artifacts.yml",
            """
            name: Release artifacts
            on:
              push:
                tags: ["v*"]
            permissions:
              contents: read
            concurrency:
              group: release-${{ github.ref_name }}
            jobs:
              build:
                permissions:
                  contents: read
                runs-on: ubuntu-latest
                steps:
                  - uses: actions/checkout@v4
                  - run: rustup target add x86_64-unknown-linux-musl
            """,
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("release rustup target add must use --toolchain with a pinned numeric toolchain", result.stderr)

    def test_floating_third_party_action_is_rejected(self) -> None:
        with workflow_dir(
            "ci.yml",
            """
            name: CI
            on: [push]
            permissions:
              contents: read
            jobs:
              test:
                runs-on: ubuntu-latest
                steps:
                  - uses: dtolnay/rust-toolchain@stable
            """,
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("third-party action must use a full commit SHA", result.stderr)

    def test_pull_request_workflow_must_not_reference_secrets(self) -> None:
        with workflow_dir(
            "ci.yml",
            """
            name: CI
            on:
              pull_request:
                branches: [main]
            permissions:
              contents: read
            jobs:
              test:
                runs-on: ubuntu-latest
                steps:
                  - run: echo "${{ secrets.GITHUB_TOKEN }}"
            """,
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("pull_request workflow must not reference secrets.*", result.stderr)

    def test_multiline_run_block_requires_strict_preamble(self) -> None:
        with workflow_dir(
            "ci.yml",
            """
            name: CI
            on: [push]
            permissions:
              contents: read
            jobs:
              test:
                runs-on: ubuntu-latest
                steps:
                  - run: |
                      echo build
                      echo test
            """,
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn('multiline run block must start with "set -euo pipefail"', result.stderr)

    def test_multiline_run_block_posix_exception_marker_is_allowed(self) -> None:
        with workflow_dir(
            "ci.yml",
            """
            name: CI
            on: [push]
            permissions:
              contents: read
            jobs:
              test:
                runs-on: ubuntu-latest
                steps:
                  - run: |
                      # m80-lint: allow-nonstrict-run
                      set -eu
                      echo posix
            """,
        ) as root:
            result = run_lint(root)

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_exception_marker_outside_run_block_does_not_bypass_strictness(self) -> None:
        with workflow_dir(
            "ci.yml",
            """
            name: CI
            on: [push]
            permissions:
              contents: read
            jobs:
              test:
                runs-on: ubuntu-latest
                steps:
                  - name: m80-lint: allow-nonstrict-run
                    run: |
                      echo build
            """,
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn('multiline run block must start with "set -euo pipefail"', result.stderr)

    def test_exception_marker_after_command_does_not_bypass_strictness(self) -> None:
        with workflow_dir(
            "ci.yml",
            """
            name: CI
            on: [push]
            permissions:
              contents: read
            jobs:
              test:
                runs-on: ubuntu-latest
                steps:
                  - run: |
                      echo build
                      # m80-lint: allow-nonstrict-run
            """,
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn('multiline run block must start with "set -euo pipefail"', result.stderr)

    def test_exception_marker_in_command_does_not_bypass_strictness(self) -> None:
        with workflow_dir(
            "ci.yml",
            """
            name: CI
            on: [push]
            permissions:
              contents: read
            jobs:
              test:
                runs-on: ubuntu-latest
                steps:
                  - run: |
                      echo "m80-lint: allow-nonstrict-run"
            """,
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn('multiline run block must start with "set -euo pipefail"', result.stderr)

    def test_multiline_run_block_allows_yaml_spacing_before_colon(self) -> None:
        with workflow_dir(
            "ci.yml",
            """
            name: CI
            on: [push]
            permissions:
              contents: read
            jobs:
              test:
                runs-on: ubuntu-latest
                steps:
                  - run : |
                      set -euo pipefail
                      echo build
            """,
        ) as root:
            result = run_lint(root)

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_multiline_run_block_chomp_variant_requires_strict_preamble(self) -> None:
        with workflow_dir(
            "ci.yml",
            """
            name: CI
            on: [push]
            permissions:
              contents: read
            jobs:
              test:
                runs-on: ubuntu-latest
                steps:
                  - run: |-
                      echo build
            """,
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn('multiline run block must start with "set -euo pipefail"', result.stderr)

    def test_multiline_pipeline_without_pipefail_is_rejected(self) -> None:
        with workflow_dir(
            "ci.yml",
            """
            name: CI
            on: [push]
            permissions:
              contents: read
            jobs:
              test:
                runs-on: ubuntu-latest
                steps:
                  - run: |
                      false | true
            """,
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("set -euo pipefail", result.stderr)

    def test_multiline_unset_variable_use_without_strict_preamble_is_rejected(self) -> None:
        with workflow_dir(
            "ci.yml",
            """
            name: CI
            on: [push]
            permissions:
              contents: read
            jobs:
              test:
                runs-on: ubuntu-latest
                steps:
                  - run: |
                      echo "$RELEASE_TAG"
            """,
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("set -euo pipefail", result.stderr)

    def test_clean_multiline_run_block_with_strict_preamble_is_allowed(self) -> None:
        with workflow_dir(
            "ci.yml",
            """
            name: CI
            on: [push]
            permissions:
              contents: read
            jobs:
              test:
                runs-on: ubuntu-latest
                steps:
                  - run: |
                      set -euo pipefail
                      echo "$RELEASE_TAG"
            """,
        ) as root:
            result = run_lint(root)

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_masked_command_substitution_in_argument_is_rejected(self) -> None:
        with workflow_dir(
            "ci.yml",
            """
            name: CI
            on: [push]
            permissions:
              contents: read
            jobs:
              test:
                runs-on: ubuntu-latest
                steps:
                  - run: |
                      set -euo pipefail
                      echo "sha=$(git rev-parse HEAD)" >> "$GITHUB_OUTPUT"
            """,
        ) as root:
            result = run_lint(root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("command substitution", result.stderr)

    def test_command_substitution_assignment_is_allowed(self) -> None:
        with workflow_dir(
            "ci.yml",
            """
            name: CI
            on: [push]
            permissions:
              contents: read
            jobs:
              test:
                runs-on: ubuntu-latest
                steps:
                  - run: |
                      set -euo pipefail
                      release_sha="$(git rev-parse HEAD)"
                      echo "sha=$release_sha" >> "$GITHUB_OUTPUT"
            """,
        ) as root:
            result = run_lint(root)

        self.assertEqual(result.returncode, 0, result.stderr)


def run_lint(workflow_dir: Path) -> subprocess.CompletedProcess[str]:
    command = ["python3", str(LINT), "--workflow-dir", str(workflow_dir)]
    policy_config = workflow_dir / "workflow-policy-scope.json"
    if policy_config.exists():
        command.extend(["--policy-config", str(policy_config)])
    timeout_budget_config = workflow_dir / "workflow-timeout-budgets.json"
    if timeout_budget_config.exists():
        command.extend(["--timeout-budget-config", str(timeout_budget_config)])
    runbook = workflow_dir / "release.md"
    command.extend(["--runbook", str(runbook)])
    return subprocess.run(
        command,
        text=True,
        capture_output=True,
        check=False,
    )


def run_authority(
    workflow_file: Path,
    *,
    repository: str = "moradology/m80",
    github_ref: str = "refs/tags/v0.1.0",
    workflow_ref: str = "moradology/m80/.github/workflows/release-artifacts.yml@refs/tags/v0.1.0",
    github_job: str = "publish-release-artifacts",
    token_source: str | None = "github.token",
    token_present: bool = True,
    receipt: Path | None = None,
    write: bool = False,
    gh_mode: str = "ok",
) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    env["GITHUB_REPOSITORY"] = repository
    env["GITHUB_REF"] = github_ref
    env["GITHUB_WORKFLOW_REF"] = workflow_ref
    env["GITHUB_JOB"] = github_job
    env["RELEASE_COMMIT"] = "0123456789abcdef0123456789abcdef01234567"
    if token_source is None:
        env.pop("M80_RELEASE_TOKEN_SOURCE", None)
    else:
        env["M80_RELEASE_TOKEN_SOURCE"] = token_source
    if token_present:
        env["GH_TOKEN"] = "ghs_test_token"
    else:
        env.pop("GH_TOKEN", None)
    with tempfile.TemporaryDirectory() as bin_dir:
        fake_gh = Path(bin_dir) / "gh"
        fake_gh.write_text(
            textwrap.dedent(
                """\
                #!/bin/sh
                case "${M80_FAKE_GH_MODE:-ok}" in
                  ok)
                    printf '{"tagName":"v0.1.0","url":"https://github.com/moradology/m80/releases/tag/v0.1.0","isDraft":false,"isPrerelease":false}\\n'
                    ;;
                  absent)
                    echo "release not found" >&2
                    exit 1
                    ;;
                  read_only)
                    echo "HTTP 403: resource not accessible by integration" >&2
                    exit 1
                    ;;
                  unavailable)
                    echo "GitHub API unavailable" >&2
                    exit 2
                    ;;
                  malformed)
                    printf '{"tagName":"not-v0.1.0","isDraft":false,"isPrerelease":false}\\n'
                    ;;
                esac
                """
            )
        )
        fake_gh.chmod(0o755)
        env["PATH"] = f"{bin_dir}:{env['PATH']}"
        env["M80_FAKE_GH_MODE"] = gh_mode
        command = ["python3", str(AUTHORITY), "--workflow-file", str(workflow_file)]
        if receipt is not None:
            command.extend(["--receipt", str(receipt)])
        if write:
            command.append("--write")
        return subprocess.run(
            command,
            env=env,
            text=True,
            capture_output=True,
            check=False,
        )


def publish_authority_workflow() -> str:
    return """
    name: Release artifacts
    on:
      push:
        tags: ["v*"]
    permissions:
      contents: read
    concurrency:
      group: release-${{ github.ref_name }}
    jobs:
      build-release-artifacts:
        permissions:
          contents: read
          id-token: write
          attestations: write
        runs-on: ubuntu-latest
        timeout-minutes: 90
        steps:
          - uses: actions/checkout@v6
      publish-release-artifacts:
        if: startsWith(github.ref, 'refs/tags/')
        permissions:
          contents: write
        runs-on: ubuntu-latest
        timeout-minutes: 30
        steps:
          - uses: actions/checkout@v6
    """


def release_artifact_origin_workflow(
    *,
    events: str = """
      push:
        tags: ["v*"]
      workflow_dispatch:
    """,
    extra_build_steps: str = "",
    extra_build_job_lines: str = "",
    publish_environment: str | None = "m80-release-publish",
) -> str:
    events_block = textwrap.indent(textwrap.dedent(events).strip(), "  ")
    extra_build_job_lines_block = textwrap.indent(
        textwrap.dedent(extra_build_job_lines).strip(), "    "
    )
    extra_build_steps_block = textwrap.indent(textwrap.dedent(extra_build_steps).strip(), "          ")
    if extra_build_steps_block:
        extra_build_steps_block = "\n" + extra_build_steps_block
    publish_environment_block = (
        "" if publish_environment is None else f"    environment: {publish_environment}\n"
    )
    return f"""
name: Release artifacts
on:
{events_block}
permissions:
  contents: read
concurrency:
  group: release-${{ github.ref_name }}
jobs:
  build-release-artifacts:
    permissions:
      contents: read
      id-token: write
      attestations: write
    runs-on: ubuntu-latest
    timeout-minutes: 90
{extra_build_job_lines_block.rstrip()}
    outputs:
      release_commit: ${{{{ steps.release-commit.outputs.sha }}}}
      release_dist_artifact_id: ${{{{ steps.upload-release-dist.outputs.artifact-id }}}}
      release_dist_artifact_name: ${{{{ steps.release-artifact-origin.outputs.name }}}}
      release_dist_producer_job: ${{{{ steps.release-artifact-origin.outputs.producer_job }}}}
      release_tag: ${{{{ steps.release-artifact-origin.outputs.release_tag }}}}
      release_upload_manifest_digest: ${{{{ steps.release-upload-manifest.outputs.digest }}}}
    steps:
      - uses: actions/checkout@v6
      - name: Prepare release build scratch dirs
        id: build-scratch
        run: |
          set -euo pipefail
          release_tmp_root="$RUNNER_TEMP/m80-release-${{GITHUB_RUN_ID}}-${{GITHUB_RUN_ATTEMPT}}-build"
          artifact_dir="$release_tmp_root/artifacts"
          dist_dir="$release_tmp_root/dist"
          test ! -e "$release_tmp_root"
          mkdir -p "$artifact_dir" "$dist_dir"
          chmod 700 "$release_tmp_root"
          test -d "$release_tmp_root"
          test ! -L "$release_tmp_root"
          owner_uid="$(stat -c '%u' "$release_tmp_root")"
          current_uid="$(id -u)"
          test "$owner_uid" = "$current_uid"
          mode_octal="$(stat -c '%a' "$release_tmp_root")"
          test "$mode_octal" = "700"
          {{
            echo "RELEASE_TMP_ROOT=$release_tmp_root"
            echo "ARTIFACT_DIR=$artifact_dir"
            echo "DIST_DIR=$dist_dir"
          }} >> "$GITHUB_ENV"
          {{
            echo "artifact_dir=$artifact_dir"
            echo "dist_dir=$dist_dir"
          }} >> "$GITHUB_OUTPUT"
      - name: Resolve release commit
        id: release-commit
        run: |
          set -euo pipefail
          echo "sha=0123456789abcdef0123456789abcdef01234567" >> "$GITHUB_OUTPUT"
      - name: Write and validate release upload manifest
        id: release-upload-manifest
        run: |
          set -euo pipefail
          digest="sha256:0123456789abcdef"
          echo "digest=$digest" >> "$GITHUB_OUTPUT"
      - name: Upload workflow artifact
        id: upload-release-dist
        uses: actions/upload-artifact@v4
        with:
          name: ${{{{ env.ARTIFACT_NAME }}}}
          path: ${{{{ steps.build-scratch.outputs.dist_dir }}}}/*
      - name: Record workflow artifact origin
        id: release-artifact-origin
        run: |
          set -euo pipefail
          {{
            echo "name=$ARTIFACT_NAME"
            echo "producer_job=build-release-artifacts"
            echo "release_tag=$GITHUB_REF_NAME"
          }} >> "$GITHUB_OUTPUT"{extra_build_steps_block}
      - name: Cleanup release build scratch dirs
        if: always()
        run: |
          set -euo pipefail
          rm -rf "$RELEASE_TMP_ROOT"
  publish-release-artifacts:
    if: startsWith(github.ref, 'refs/tags/')
    needs: build-release-artifacts
    permissions:
      contents: write
    runs-on: ubuntu-latest
    timeout-minutes: 30
{publish_environment_block.rstrip()}
    steps:
      - uses: actions/checkout@v6
      - name: Prepare release publish scratch dirs
        id: publish-scratch
        run: |
          set -euo pipefail
          release_tmp_root="$RUNNER_TEMP/m80-release-${{GITHUB_RUN_ID}}-${{GITHUB_RUN_ATTEMPT}}-publish"
          upload_dir="$release_tmp_root/upload"
          prepublish_dir="$release_tmp_root/prepublish"
          redownload_dir="$release_tmp_root/redownload"
          test ! -e "$release_tmp_root"
          mkdir -p "$upload_dir" "$prepublish_dir" "$redownload_dir"
          chmod 700 "$release_tmp_root"
          test -d "$release_tmp_root"
          test ! -L "$release_tmp_root"
          owner_uid="$(stat -c '%u' "$release_tmp_root")"
          current_uid="$(id -u)"
          test "$owner_uid" = "$current_uid"
          mode_octal="$(stat -c '%a' "$release_tmp_root")"
          test "$mode_octal" = "700"
          {{
            echo "RELEASE_TMP_ROOT=$release_tmp_root"
            echo "UPLOAD_DIR=$upload_dir"
            echo "PREPUBLISH_DIR=$prepublish_dir"
            echo "REDOWNLOAD_DIR=$redownload_dir"
          }} >> "$GITHUB_ENV"
          {{
            echo "upload_dir=$upload_dir"
            echo "prepublish_dir=$prepublish_dir"
            echo "redownload_dir=$redownload_dir"
          }} >> "$GITHUB_OUTPUT"
      - uses: actions/download-artifact@v4
        with:
          artifact-ids: ${{{{ needs.build-release-artifacts.outputs.release_dist_artifact_id }}}}
          merge-multiple: true
          path: ${{{{ steps.publish-scratch.outputs.upload_dir }}}}
      - name: Verify workflow artifact origin handoff
        env:
          EXPECTED_ARTIFACT_ID: ${{{{ needs.build-release-artifacts.outputs.release_dist_artifact_id }}}}
          EXPECTED_ARTIFACT_NAME: ${{{{ needs.build-release-artifacts.outputs.release_dist_artifact_name }}}}
          EXPECTED_PRODUCER_JOB: ${{{{ needs.build-release-artifacts.outputs.release_dist_producer_job }}}}
          EXPECTED_RELEASE_TAG: ${{{{ needs.build-release-artifacts.outputs.release_tag }}}}
          EXPECTED_MANIFEST_DIGEST: ${{{{ needs.build-release-artifacts.outputs.release_upload_manifest_digest }}}}
        run: |
          set -euo pipefail
          test -n "$EXPECTED_ARTIFACT_ID"
          test "$EXPECTED_ARTIFACT_NAME" = "$ARTIFACT_NAME"
          test "$EXPECTED_PRODUCER_JOB" = "build-release-artifacts"
          test "$EXPECTED_RELEASE_TAG" = "$GITHUB_REF_NAME"
          manifest_sha="$(sha256sum "$UPLOAD_DIR/m80-release-upload-manifest.json" | awk '{{print $1}}')"
          observed_digest="sha256:$manifest_sha"
          test "$observed_digest" = "$EXPECTED_MANIFEST_DIGEST"
      - name: Verify release upload manifest before upload
        run: |
          set -euo pipefail
          scripts/release_upload_manifest.py --dist-dir "$UPLOAD_DIR" --release-tag "$GITHUB_REF_NAME"
      - name: Write pre-upload release readiness decision
        run: |
          set -euo pipefail
          scripts/release_readiness_decision.py \\
            --stage pre-upload \\
            --out "$UPLOAD_DIR/m80-release-readiness-decision.json"
      - name: Validate publish decision receipt
        run: |
          set -euo pipefail
          scripts/release_publish_receipt.py \\
            --readiness-decision "$UPLOAD_DIR/m80-release-readiness-decision.json"
      - name: Audit repository release protections before mutation
        env:
          GH_TOKEN: ${{{{ github.token }}}}
        run: |
          set -euo pipefail
          scripts/repository_protection_audit.py \\
            --repository "$GITHUB_REPOSITORY" \\
            --branch main \\
            --tag-pattern "v*" \\
            --environment m80-release-publish \\
            --out "$UPLOAD_DIR/m80-repository-protection-audit.json" \\
            --write
      - name: Upload repository protection audit
        uses: actions/upload-artifact@v4
        with:
          name: m80-repository-protection-audit-${{{{ github.run_id }}}}
          path: ${{{{ steps.publish-scratch.outputs.upload_dir }}}}/m80-repository-protection-audit.json
          if-no-files-found: error
      - name: Approve latest promotion
        run: |
          set -euo pipefail
          scripts/release_readiness_decision.py \\
            --stage pre-latest \\
            --out "$UPLOAD_DIR/m80-release-readiness-pre-latest-decision.json"
          scripts/release_latest_promotion.py \\
            --release-tag "$GITHUB_REF_NAME" \\
            --release-list "$REDOWNLOAD_DIR/github-releases-before-latest.json" \\
            --publish-decision "$UPLOAD_DIR/m80-release-publish-decision.json" \\
            --proof-ledger "$UPLOAD_DIR/m80-release-proof-ledger.jsonl" \\
            --upload-manifest "$UPLOAD_DIR/m80-release-upload-manifest.json" \\
            --remote-inventory "$REDOWNLOAD_DIR/m80-release-remote-assets.json" \\
            --out "$UPLOAD_DIR/m80-latest-promotion-decision.json" \\
            --write
      - name: Upload latest promotion decision
        uses: actions/upload-artifact@v4
        with:
          name: m80-latest-promotion-decision-${{{{ github.run_id }}}}
          path: ${{{{ steps.publish-scratch.outputs.upload_dir }}}}/m80-latest-promotion-decision.json
          if-no-files-found: error
      - name: Upload latest rollback receipt
        if: ${{{{ hashFiles('docs/operations/release-latest-rollback-receipt.json') != '' }}}}
        uses: actions/upload-artifact@v4
        with:
          name: m80-latest-rollback-receipt-${{{{ github.run_id }}}}
          path: docs/operations/release-latest-rollback-receipt.json
          if-no-files-found: error
      - name: Upload release readiness decision
        uses: actions/upload-artifact@v4
        with:
          name: m80-release-readiness-decision-${{{{ github.run_id }}}}
          path: ${{{{ steps.publish-scratch.outputs.upload_dir }}}}/m80-release-readiness-decision.json
          if-no-files-found: error
      - name: Cleanup release publish scratch dirs
        if: always()
        run: |
          set -euo pipefail
          rm -rf "$RELEASE_TMP_ROOT"
"""


def latest_freshness_workflow(
    *,
    events: str = """
      schedule:
        - cron: "17 4 * * *"
      workflow_dispatch:
    """,
    cancel_in_progress: str = "false",
    job_permissions: str = "contents: read",
    runs_on: str = "ubuntu-latest",
    command: str = (
        "python3 scripts/release_freshness.py --json "
        "--proof-out /tmp/m80-latest-freshness/m80-latest-freshness-proof.json"
    ),
    upload_artifact: bool = True,
    upload_drift_artifact: bool = True,
    upload_stderr_artifact: bool = True,
    publish_latest_proof: bool = True,
) -> str:
    events_block = textwrap.indent(textwrap.dedent(events).strip(), "  ")
    stderr_artifact = (
        "\n                      /tmp/m80-latest-freshness/m80-latest-freshness.stderr"
        if upload_stderr_artifact
        else ""
    )
    drift_artifact = (
        "\n                      /tmp/m80-latest-freshness/m80-latest-freshness-drift.json"
        if upload_drift_artifact
        else ""
    )
    upload_step = (
        textwrap.indent(
            textwrap.dedent(
                f"""
                - name: Upload latest freshness evidence
                  if: always()
                  uses: actions/upload-artifact@v4
                  with:
                    path: |
                      /tmp/m80-latest-freshness/m80-latest-freshness-proof.json
                      /tmp/m80-latest-freshness/m80-latest-freshness.stdout{drift_artifact}{stderr_artifact}
                    if-no-files-found: error
                """
            ).strip(),
            "          ",
        )
        if upload_artifact
        else ""
    )
    publish_job = (
        textwrap.indent(
            textwrap.dedent(
                """
                publish-latest-freshness:
                  needs: hostless-public-freshness
                  if: ${{ needs.hostless-public-freshness.result == 'success' }}
                  runs-on: ubuntu-latest
                  timeout-minutes: 5
                  permissions:
                    contents: write
                  steps:
                    - uses: actions/download-artifact@v6
                      with:
                        name: m80-latest-freshness-${{ github.run_id }}
                        path: /tmp/m80-latest-freshness-publish
                    - name: Publish latest freshness proof release asset
                      env:
                        GH_TOKEN: ${{ github.token }}
                      run: |
                        set -euo pipefail
                        proof=/tmp/m80-latest-freshness-publish/m80-latest-freshness-proof.json
                        tag="$(python3 - "$proof" <<'PY'
                        import json
                        import sys
                        proof = json.load(open(sys.argv[1], encoding="utf-8"))
                        print(proof["resolved_tag"])
                        PY
                        )"
                        gh release upload "$tag" "$proof#m80-latest-freshness-proof.json" --clobber --repo "$GITHUB_REPOSITORY"
                """
            ).strip(),
            "  ",
        )
        if publish_latest_proof
        else ""
    )
    return f"""
name: Latest freshness
on:
{events_block}
permissions:
  contents: read
concurrency:
  group: latest-freshness-public
  cancel-in-progress: {cancel_in_progress}
jobs:
  hostless-public-freshness:
    runs-on: {runs_on}
    timeout-minutes: 10
    permissions:
      {job_permissions}
    steps:
      - uses: actions/checkout@v6
      - name: Run public latest freshness verifier
        run: |
          set -euo pipefail
          mkdir -p /tmp/m80-latest-freshness
          {command}
{upload_step}
{publish_job}
"""


def ci_workflow(*, diff_check_step: str | None = None) -> str:
    step = ci_diff_check_step() if diff_check_step is None else diff_check_step
    step_block = textwrap.indent(textwrap.dedent(step).strip(), "      ")
    if step_block:
        step_block = "\n" + step_block
    return f"""
name: CI
on:
  push:
    branches: [main]
  pull_request:
    branches: [main]
permissions:
  contents: read
jobs:
  test:
    runs-on: ubuntu-latest
    timeout-minutes: 45
    steps:
      - uses: actions/checkout@v6
        with:
          fetch-depth: 0{step_block}
      - name: cargo build
        run: cargo build --workspace
"""


def ci_diff_check_step() -> str:
    return """
    - name: changed-line whitespace check
      env:
        BASE_SHA: ${{ github.event_name == 'pull_request' && github.event.pull_request.base.sha || github.event.before }}
        EVENT_NAME: ${{ github.event_name }}
      run: |
        set -euo pipefail
        if [ "$EVENT_NAME" = "push" ] && [ "$BASE_SHA" = "0000000000000000000000000000000000000000" ]; then
          BASE_SHA="$(git rev-list --max-parents=0 HEAD)"
        fi
        git cat-file -e "$BASE_SHA^{commit}"
        git diff --check "$BASE_SHA"...HEAD
    """


def privileged_e2e_workflow() -> str:
    return """
name: Privileged E2E
on:
  workflow_dispatch:
    inputs:
      runner_label:
        description: Self-hosted runner label to target
        required: false
        default: kvm
      pull_number:
        description: Pull request number whose head ref should be fetched before target_sha checkout
        required: false
        default: ""
      target_sha:
        description: Exact commit SHA to check out after fetching the public repository or PR ref
        required: false
        default: ""
  pull_request:
    types: [opened, synchronize, reopened, labeled]
permissions:
  contents: read
env:
  M80_E2E_RUNNER_LABEL: ${{ inputs.runner_label || vars.M80_E2E_RUNNER_LABEL || 'kvm' }}
  M80_E2E_PULL_NUMBER: ${{ inputs.pull_number || '' }}
  M80_E2E_TARGET_SHA: ${{ inputs.target_sha || '' }}
jobs:
  privileged-e2e:
    runs-on:
      - self-hosted
      - ${{ inputs.runner_label || vars.M80_E2E_RUNNER_LABEL || 'kvm' }}
    timeout-minutes: 120
    steps:
      - name: Checkout public repository
        run: |
          set -euo pipefail
          if [[ -n "$M80_E2E_TARGET_SHA" ]]; then
            if ! [[ "$M80_E2E_TARGET_SHA" =~ ^[0-9a-fA-F]{40}$ ]]; then
              exit 2
            fi
            target_sha="${M80_E2E_TARGET_SHA,,}"
            if [[ -n "$M80_E2E_PULL_NUMBER" ]]; then
              if ! [[ "$M80_E2E_PULL_NUMBER" =~ ^[1-9][0-9]*$ ]]; then
                exit 2
              fi
              git -C "$GITHUB_WORKSPACE" fetch --no-tags --prune --no-recurse-submodules origin \
                "+refs/pull/$M80_E2E_PULL_NUMBER/head:refs/remotes/origin/pr/$M80_E2E_PULL_NUMBER"
            else
              git -C "$GITHUB_WORKSPACE" fetch --no-tags --prune --no-recurse-submodules origin "$target_sha"
            fi
            git -C "$GITHUB_WORKSPACE" checkout --force "$target_sha"
            checked_out_sha="$(git -C "$GITHUB_WORKSPACE" rev-parse HEAD)"
          fi
      - name: Validate operator inputs
        run: |
          set -euo pipefail
          if ! [[ "$M80_E2E_RUNNER_LABEL" =~ ^[A-Za-z0-9_.-]+$ ]]; then
            exit 2
          fi
          if [[ -n "$M80_E2E_TARGET_SHA" ]] && ! [[ "$M80_E2E_TARGET_SHA" =~ ^[0-9a-fA-F]{40}$ ]]; then
            exit 2
          fi
          if [[ -n "$M80_E2E_PULL_NUMBER" ]] && ! [[ "$M80_E2E_PULL_NUMBER" =~ ^[1-9][0-9]*$ ]]; then
            exit 2
          fi
      - name: Check KVM runner substrate
        run: |
          set -euo pipefail
          printf 'M80_E2E_RUNNER_LABEL=%s\\n' "$M80_E2E_RUNNER_LABEL"
          printf 'M80_E2E_TARGET_SHA=%s\\n' "$M80_E2E_TARGET_SHA"
          checked_out_sha="$(git rev-parse HEAD)"
          printf 'M80_E2E_CHECKED_OUT_SHA=%s\\n' "$checked_out_sha"
"""


class workflow_dir:
    def __init__(
        self,
        name: str,
        body: str,
        *,
        scope: str | None = None,
        policy_entries: list[dict[str, str]] | None = None,
        timeout_entries: list[dict[str, object]] | None = None,
        runbook_text: str | None = None,
        write_policy: bool = True,
        write_timeout_budget: bool = True,
    ) -> None:
        self.name = name
        self.body = textwrap.dedent(body).strip() + "\n"
        self.scope = infer_workflow_scope(name) if scope is None else scope
        self.policy_entries = policy_entries
        self.timeout_entries = timeout_entries
        self.runbook_text = runbook_text
        self.write_policy = write_policy
        self.write_timeout_budget = write_timeout_budget
        self.temp: tempfile.TemporaryDirectory[str] | None = None

    def __enter__(self) -> Path:
        self.temp = tempfile.TemporaryDirectory()
        root = Path(self.temp.name)
        (root / self.name).write_text(self.body)
        if self.write_policy:
            entries = (
                [{"path": self.name, "scope": self.scope}]
                if self.policy_entries is None
                else self.policy_entries
            )
            (root / "workflow-policy-scope.json").write_text(
                json.dumps({"schema_version": 1, "workflows": entries}, indent=2)
                + "\n"
            )
        if self.write_timeout_budget:
            entries = (
                infer_timeout_budget_entries(self.name, self.body)
                if self.timeout_entries is None
                else self.timeout_entries
            )
            (root / "workflow-timeout-budgets.json").write_text(
                json.dumps({"schema_version": 1, "jobs": entries}, indent=2)
                + "\n"
            )
        if self.runbook_text is not None:
            (root / "release.md").write_text(textwrap.dedent(self.runbook_text).strip() + "\n")
        return root

    def __exit__(self, *args: object) -> None:
        assert self.temp is not None
        self.temp.cleanup()


def infer_workflow_scope(name: str) -> str:
    if name == "latest-freshness.yml":
        return "latest-freshness"
    if "proof" in name:
        return "proof"
    if any(token in name for token in ["release", "latest", "freshness", "publish"]):
        return "release-authority"
    return "ordinary-ci"


def infer_timeout_budget_entries(name: str, body: str) -> list[dict[str, object]]:
    lines = body.splitlines()
    entries: list[dict[str, object]] = []
    for job_id, start, end in job_blocks_from_text(lines):
        block = lines[start:end]
        entries.append(
            {
                "path": name,
                "job": job_id,
                "timeout_minutes": infer_timeout_minutes(block),
            }
        )
    return entries


def job_blocks_from_text(lines: list[str]) -> list[tuple[str, int, int]]:
    jobs_start = next((i for i, line in enumerate(lines) if line == "jobs:"), None)
    if jobs_start is None:
        return []
    jobs: list[tuple[str, int, int]] = []
    current: tuple[str, int] | None = None
    for index in range(jobs_start + 1, len(lines)):
        line = lines[index]
        if line and not line.startswith(" "):
            break
        match = re.match(r"^  ([A-Za-z0-9_-]+):\s*(?:#.*)?$", line)
        if match is None:
            continue
        if current is not None:
            jobs.append((current[0], current[1], index))
        current = (match.group(1), index)
    if current is not None:
        jobs.append((current[0], current[1], len(lines)))
    return jobs


def infer_timeout_minutes(lines: list[str]) -> int:
    for line in lines:
        stripped = line.strip()
        if stripped.startswith("timeout-minutes:"):
            value = stripped.split(":", 1)[1].split("#", 1)[0].strip()
            if value.isdigit():
                return int(value)
        marker = re.search(r"m80-lint:\s*reusable-timeout-minutes=([0-9]+)\b", stripped)
        if marker is not None:
            return int(marker.group(1))
    return 30


if __name__ == "__main__":
    unittest.main()
