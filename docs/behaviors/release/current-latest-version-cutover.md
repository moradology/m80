# Current Latest Version Cutover

`m80-o3uh9.16.8` cuts the workspace package identity over to the next stable
public repair version. The release workflow only packages a tag when the tag
equals `v<workspace.package.version>`, and the current public latest repair
must supersede the `v0.2.7` installer handoff that did not preserve proof-cache
material.
`v0.2.8` was published but not promoted to latest because the workflow generated
the no-auth public-access receipt before GitHub latest pointed at the candidate;
`v0.2.9` carries the workflow ordering repair.

The current repair source therefore uses workspace package version `0.2.9`.
The matching stable tag is `v0.2.9`; `v0.2.7` is intentionally rejected as an
old-release backfill candidate by `scripts/current_latest_repair_preflight.py`.

The release runbook documents this as the real source-state expectation rather
than a fixture-only value. Dev builds still render as `<package-version>-dev`,
so an unreleased local build now reports `0.2.9-dev` and remains barred from
using mutable `releases/latest` implicitly.

Fixture and release-script coverage uses `v0.2.9` as the packageable release
tag. Rust installer-layout fixtures derive their release tag from
`CARGO_PKG_VERSION`, so the bundled layout tests also follow the same cutover.
Tests may still use other tags only when they are explicitly testing mismatch,
downgrade, stale-tag, or historical-proof handling.
