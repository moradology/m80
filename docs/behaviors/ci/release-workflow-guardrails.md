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
- Release artifact bytes cross from build to publish only through GitHub
  workflow artifact identity. The build job records the workflow artifact id,
  fixed artifact name, producing job id, source commit, release tag, and release
  upload manifest digest. The publish job downloads by the recorded artifact id,
  checks the recorded name, producer, tag, commit, and manifest digest, and only
  then runs upload authority checks.
- Release publish does not accept runner-local dist paths, cache contents,
  dispatch-input artifact overrides, manual artifact URLs, or mutable latest
  aliases as release-byte inputs. The release upload manifest is the byte list
  checked after download and before any public release mutation.
- Runner-local release scratch directories are not trust anchors. Build and
  publish jobs create per-run roots under `$RUNNER_TEMP`, refuse preexisting
  scratch roots, symlink roots, wrong-owner roots, and group/world-writable
  roots, and clean those roots only as best-effort diagnostics after artifacts
  have been uploaded or checked.

## Enforcement

`scripts/lint-github-workflows.py` checks the repository workflows for broad
write permissions, release/latest workflows without concurrency, floating
third-party action refs, `secrets.*` references in pull-request workflows, and
multi-line `run:` blocks that omit the strict shell prelude.

`CI` installs the host test tools, then runs the m80 linter and the pinned
actionlint syntax/run-block gate on every push and pull request before the
normal Rust build/test/clippy sequence. The m80 linter's negative fixture suite
lives in `scripts/test-workflow-policy.py`.

For `release-artifacts.yml`, the m80 linter also checks the artifact-origin
handoff: build outputs must expose the recorded artifact id/name, producer job,
release tag, source commit, and release upload manifest digest; publish must
download by the recorded artifact id and must reject manual artifact overrides,
cache reuse, runner-local dist reuse, and missing or mismatched handoff fields.
It also rejects fixed `/tmp/m80-release-*` paths and requires the build and
publish jobs to validate their `$RUNNER_TEMP` scratch roots before producing or
consuming release bytes.

## Verification

- `crates/m80-cli/tests/workflow_policy.rs` runs the workflow linter and its
  negative fixture suite from the normal Rust integration-test surface.
- `scripts/test-workflow-policy.py` proves the linter rejects missing
  release/latest concurrency, non-publish write tokens, `write-all`, floating
  third-party action refs, `secrets.*` in pull-request workflows, and workflow
  run blocks that rely on GitHub's implicit shell flags. It also covers manual
  artifact override inputs, cache path injection, runner-local dist reuse, wrong
  artifact id handoff, and the accepted build-to-publish workflow artifact
  handoff. The same suite covers fixed `/tmp` literals, preexisting scratch
  path reuse, symlink staging roots, unsafe mode/owner checks, and clean
  `$RUNNER_TEMP` usage.
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
