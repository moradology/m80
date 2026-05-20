# Release Bundle Runbook

This runbook records the release workflow authority boundary for the Linux
bundle path.

## Workflow Stages

`CI` runs `python3 scripts/lint-github-workflows.py` on every push and pull
request. The linter rejects broad workflow permissions, release workflows
without a concurrency group, floating third-party action refs, and
`pull_request` workflows that reference `secrets.*`. It also rejects
multi-line workflow `run:` blocks that do not start with `set -euo pipefail`,
unless the block carries the documented `m80-lint: allow-nonstrict-run`
exception marker.

`Release artifacts` uses a concurrency group keyed by the GitHub ref name:
`release-artifacts-${{ github.ref_name }}`. For tag pushes, that key is the
release tag. For manual dispatches, it is the selected ref.

`build-release-artifacts` has `contents: read`. It checks out the repository,
installs the pinned Rust toolchain from this repo's toolchain policy, builds
the release artifacts with `cargo --locked`, records apt package versions,
packages the release bundle plus `m80-release-assets.json`,
`m80-bootstrap-selector.tsv`, and `m80-release-build.json`, and uploads only a
workflow artifact.

`publish-release-artifacts` runs only for tag refs after the build job
finishes. It is the only job with `contents: write`. It downloads the workflow
artifact, uploads the exact files to the matching GitHub Release with
`gh release upload`, re-downloads those public assets, and runs
`scripts/verify-release-bundle.py --verify-sidecars` against the downloaded
bundle so the published asset index is checked against the uploaded tarball,
metadata, bootstrap selector, checksums, and installer before any later
latest-promotion lane can trust it.

## Build Manifest

Every release dist includes `m80-release-build.json` and
`m80-release-build.json.sha256`. Inspect it before publication or when
debugging input drift:

```sh
jq . /tmp/m80-release-dist/m80-release-build.json
sha256sum -c /tmp/m80-release-dist/m80-release-build.json.sha256
```

The manifest must name the release tag, source commit, Rust toolchain, host and
guest target triples, `Cargo.lock` digest, builder identity, builder OS image,
and either apt package versions or a container digest. Publication is blocked
when the manifest's tag, commit, Rust toolchain, package version, bundle
metadata hash, or `Cargo.lock` hash does not match the rest of the dist.
The current GitHub workflow records apt package versions. If a future release
builder switches to a containerized builder, its manifest must record an
immutable `sha256:<64 lowercase hex>` OCI digest; tags and repository names are
not accepted as builder provenance.

For pull requests, CI runs the same package/verify script tests without publish
authority. Those fixture packages must emit the same manifest shape as a tag
release, so a schema drift fails before the protected release workflow runs.

## Authority Boundary

Top-level workflow permissions stay read-only. A write-capable token is scoped
to the publish job because that is the only stage that mutates GitHub release
state. Build, verify, PR, and documentation jobs do not receive write tokens.

Pull-request workflows must not reference `secrets.*`. Jobs that need the
standard GitHub token use `github.token`, which remains governed by the
workflow/job `permissions` map.

`actions/*` refs are the documented trusted first-party exception and may use
GitHub's major-version tags. Third-party actions must be pinned by full
40-character commit SHA. Prefer eliminating third-party actions from the
release path when a local shell command is clear enough.

Release workflows must keep Rust inputs pinned. The workflow policy linter
rejects floating `rustup toolchain install` values, `rustup target add` without
a pinned `--toolchain`, and release `cargo` build/test/clippy/install
invocations that omit `--locked`.

Multi-line workflow `run:` blocks are treated as release-authority shell
scripts. The local workflow strictness check is the same command:
`python3 scripts/lint-github-workflows.py`. Run it beside the pinned
`actionlint` lane once that syntax/run-block gate is installed. The m80 linter
catches project authority rules and the strict shell prelude; actionlint catches
GitHub expression and shell syntax drift.
