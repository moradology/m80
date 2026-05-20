#!/usr/bin/env python3
"""Tests for scripts/lint-github-workflows.py."""

from __future__ import annotations

import subprocess
from pathlib import Path
import tempfile
import textwrap
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
LINT = REPO_ROOT / "scripts" / "lint-github-workflows.py"


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
                steps:
                  - uses: actions/checkout@v4
            """,
        ) as root:
            result = run_lint(root)

        self.assertEqual(result.returncode, 0, result.stderr)

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
