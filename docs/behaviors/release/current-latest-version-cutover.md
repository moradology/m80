# Current Latest Version Cutover

`m80-o3uh9.16.8` cuts the workspace package identity over to the next stable
public repair version. The release workflow only packages a tag when the tag
equals `v<workspace.package.version>`, and the current public latest repair
must supersede the `v0.2.7` installer handoff that did not preserve proof-cache
material.
`v0.2.8` was published but not promoted to latest because the workflow generated
the no-auth public-access receipt before GitHub latest pointed at the candidate;
`v0.2.9` carries the workflow ordering repair and public proof-cache handoff.
`v0.2.10` carries the selector-path repair that makes default installs write
the host `/etc/m80` selector state while custom install-root proofs remain
root-local and inspectable. `v0.2.11` carries the path-budget hardening found
while proving a public install can wrap an actual process from `/tank/tmp`.
`v0.2.12` built the redacted `m80 bug-report` support bundle, but its publish
job failed before release mutation because the downloaded workflow artifact was
nested one directory below the verified upload root. `v0.2.13` carried the same
support bundle plus the flattened artifact-download handoff, but its publish
job failed before release mutation because GitHub's ruleset list API omitted
the tag ref conditions needed by the repository protection audit. `v0.2.14`
hydrated the ruleset detail before evaluating release-tag protections, then
failed before release mutation because the workflow token could not read the
branch-protection endpoint. `v0.2.15` accepts an active branch ruleset with
required status checks as the workflow-readable proof for `main`, then failed
before release mutation because the publish-authority check required the
release to already exist. `v0.2.16` treats a "release not found" metadata probe
as the expected pre-create state while still failing on unreadable API errors,
then published validated release assets but failed latest promotion because the
hosted workflow still required an external real-KVM receipt. `v0.2.17` moves
that real-KVM proof to the closeout/freshness evidence path instead of the
GitHub-hosted latest-promotion gate.

The current repair source therefore uses workspace package version `0.2.17`.
The matching stable tag is `v0.2.17`; tags at or before the existing public
latest `v0.2.11` are intentionally rejected as old-release backfill candidates
by `scripts/current_latest_repair_preflight.py`.

The release runbook documents this as the real source-state expectation rather
than a fixture-only value. Dev builds still render as `<package-version>-dev`,
so an unreleased local build now reports `0.2.17-dev` and remains barred from
using mutable `releases/latest` implicitly.

Fixture and release-script coverage uses `v0.2.17` as the packageable release
tag. Rust installer-layout fixtures derive their release tag from
`CARGO_PKG_VERSION`, so the bundled layout tests also follow the same cutover.
Tests may still use other tags only when they are explicitly testing mismatch,
downgrade, stale-tag, or historical-proof handling.
