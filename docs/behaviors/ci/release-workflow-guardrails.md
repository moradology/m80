# Release Workflow Guardrails

Behavior capture for bead `m80-o3uh9.13.3`.

## Contract

Release and latest-promotion workflows must be auditable before they can mutate
public release state.

- Top-level workflow permissions stay read-only.
- Build and verification jobs use `contents: read`.
- The only release workflow job allowed to request `contents: write` is a
  tag-gated publish job.
- Release/latest workflows declare a concurrency group keyed by the release tag,
  GitHub ref, or latest-promotion target.
- Pull-request workflows do not reference `secrets.*`.
- `actions/*` refs are the trusted first-party exception documented in
  `docs/runbook/release-bundle.md`; third-party actions use a full 40-character
  commit SHA.
- Multi-line workflow `run:` blocks start with `set -euo pipefail`, so
  pipeline failures and unset variables fail in the block itself. A block may
  opt out only with the `m80-lint: allow-nonstrict-run` marker when a documented
  POSIX or non-bash shell contract requires it.

## Enforcement

`scripts/lint-github-workflows.py` checks the repository workflows for broad
write permissions, release/latest workflows without concurrency, floating
third-party action refs, `secrets.*` references in pull-request workflows, and
multi-line `run:` blocks that omit the strict shell prelude.

`CI` runs that linter on every push and pull request before the normal Rust
build/test/clippy sequence. The linter's negative fixture suite lives in
`scripts/test-workflow-policy.py`.

## Verification

- `crates/m80-cli/tests/workflow_policy.rs` runs the workflow linter and its
  negative fixture suite from the normal Rust integration-test surface.
- `scripts/test-workflow-policy.py` proves the linter rejects missing
  release/latest concurrency, non-publish write tokens, `write-all`, floating
  third-party action refs, `secrets.*` in pull-request workflows, and workflow
  run blocks that rely on GitHub's implicit shell flags.
