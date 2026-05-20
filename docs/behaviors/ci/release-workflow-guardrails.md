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
- The pinned workflow syntax/run-block lint runner is `scripts/run-actionlint.py`. It
  pins `rhysd/actionlint` `v1.7.12` for Linux x86_64 and verifies
  `actionlint_1.7.12_linux_amd64.tar.gz` before extracting the binary. The
  runner never falls back to an unverified `actionlint` from `PATH`; CI uses
  this runner rather than a floating install. CI installs `shellcheck` before
  running actionlint so inline workflow `run:` blocks receive shell diagnostics
  through the same pinned actionlint path.

## Enforcement

`scripts/lint-github-workflows.py` checks the repository workflows for broad
write permissions, release/latest workflows without concurrency, floating
third-party action refs, `secrets.*` references in pull-request workflows, and
multi-line `run:` blocks that omit the strict shell prelude.

`CI` installs the host test tools, then runs the m80 linter and the pinned
actionlint syntax/run-block gate on every push and pull request before the
normal Rust build/test/clippy sequence. The m80 linter's negative fixture suite
lives in `scripts/test-workflow-policy.py`.

## Verification

- `crates/m80-cli/tests/workflow_policy.rs` runs the workflow linter and its
  negative fixture suite from the normal Rust integration-test surface.
- `scripts/test-workflow-policy.py` proves the linter rejects missing
  release/latest concurrency, non-publish write tokens, `write-all`, floating
  third-party action refs, `secrets.*` in pull-request workflows, and workflow
  run blocks that rely on GitHub's implicit shell flags.
- `scripts/test-actionlint-runner.py` proves the pinned actionlint runner
  accepts verified metadata, recreates a missing cached binary from the verified
  archive, rejects checksum mismatches, rejects unsupported platforms, reports
  download failures, and refuses archives without an `actionlint` binary.
- `scripts/test-actionlint-fixtures.py` proves the pinned actionlint binary
  accepts a valid workflow fixture, rejects stale `needs`, invalid GitHub
  expressions, invalid event syntax, duplicate job ids, and shellcheck failures
  in inline bash run blocks, accepts a strict bash run block, accepts a
  documented non-bash exception, and tolerates concurrent CLI invocations
  sharing one cache.
