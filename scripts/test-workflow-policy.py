#!/usr/bin/env python3
"""Tests for scripts/lint-github-workflows.py."""

from __future__ import annotations

import os
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

    def test_release_build_attestation_permissions_are_allowed(self) -> None:
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
                  id-token: write
                  attestations: write
                runs-on: ubuntu-latest
                timeout-minutes: 90
                steps:
                  - uses: actions/checkout@v4
                  - uses: actions/attest@v4
                    with:
                      subject-name: m80-release-integrity.json
                      subject-digest: sha256:0123456789abcdef
              publish-release-artifacts:
                permissions:
                  contents: write
                runs-on: ubuntu-latest
                timeout-minutes: 30
                steps:
                  - uses: actions/checkout@v4
            """,
        ) as root:
            result = run_lint(root)

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_publish_authority_policy_allows_clean_publish_context(self) -> None:
        with workflow_dir("release-artifacts.yml", publish_authority_workflow()) as root:
            result = run_authority(root / "release-artifacts.yml")

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_publish_authority_policy_rejects_wrong_repository(self) -> None:
        with workflow_dir("release-artifacts.yml", publish_authority_workflow()) as root:
            result = run_authority(
                root / "release-artifacts.yml",
                repository="example/m80",
                workflow_ref="example/m80/.github/workflows/release-artifacts.yml@refs/tags/v0.1.0",
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("repository mismatch", result.stderr)

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
            result = run_authority(root / "release-artifacts.yml", token_present=False)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("token env GH_TOKEN missing", result.stderr)

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
    return subprocess.run(
        ["python3", str(LINT), "--workflow-dir", str(workflow_dir)],
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
) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    env["GITHUB_REPOSITORY"] = repository
    env["GITHUB_REF"] = github_ref
    env["GITHUB_WORKFLOW_REF"] = workflow_ref
    env["GITHUB_JOB"] = github_job
    if token_source is None:
        env.pop("M80_RELEASE_TOKEN_SOURCE", None)
    else:
        env["M80_RELEASE_TOKEN_SOURCE"] = token_source
    if token_present:
        env["GH_TOKEN"] = "ghs_test_token"
    else:
        env.pop("GH_TOKEN", None)
    return subprocess.run(
        ["python3", str(AUTHORITY), "--workflow-file", str(workflow_file)],
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
"""


class workflow_dir:
    def __init__(self, name: str, body: str) -> None:
        self.name = name
        self.body = textwrap.dedent(body).strip() + "\n"
        self.temp: tempfile.TemporaryDirectory[str] | None = None

    def __enter__(self) -> Path:
        self.temp = tempfile.TemporaryDirectory()
        root = Path(self.temp.name)
        (root / self.name).write_text(self.body)
        return root

    def __exit__(self, *args: object) -> None:
        assert self.temp is not None
        self.temp.cleanup()


if __name__ == "__main__":
    unittest.main()
