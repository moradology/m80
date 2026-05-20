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
`m80-bootstrap-selector.tsv`, and `m80-release-build.json`, writes
`m80-release-upload-manifest.json` after release-integrity attestation metadata
and hostless proof evidence exist, and uploads only a workflow artifact.

`publish-release-artifacts` runs only for tag refs after the build job
finishes. It is the only job with `contents: write`. It downloads the workflow
artifact, validates `m80-release-upload-manifest.json`, derives the
`gh release upload` path list and `gh release download --pattern` list from
that manifest, re-downloads those public assets, verifies the redownload
directory contains exactly the manifest's public asset set, and runs
`scripts/verify-release-bundle.py --verify-sidecars` against the downloaded
bundle so the published asset index is checked against the uploaded tarball,
metadata, bootstrap selector, checksums, and installer before any later
latest-promotion lane can trust it.

The upload manifest itself and `m80-quickstart-proof-hostless.json` stay
workflow-only artifacts. Public proof is the release-integrity predicate,
attestation bundle, and normalized attestation metadata listed in the manifest.

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
`actionlint` lane. The m80 linter catches project authority rules and the
strict shell prelude; actionlint catches GitHub expression, workflow graph, and
inline run-block shellcheck drift. Install `shellcheck` before running
actionlint locally if you are changing workflow `run:` blocks.

The pinned local actionlint command is:

```sh
python3 scripts/run-actionlint.py --workflow-dir .github/workflows
```

The runner currently pins `rhysd/actionlint` `v1.7.12` for Linux x86_64:
`actionlint_1.7.12_linux_amd64.tar.gz` with sha256
`8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8`.
It downloads from the public GitHub release without credentials, verifies the
archive before extraction, and never falls back to an unverified `actionlint`
from `PATH`.

To refresh the pin, inspect the upstream release, update the version, URL, and
sha256 constants in `scripts/run-actionlint.py`, then run:

```sh
python3 scripts/test-actionlint-runner.py
python3 scripts/test-actionlint-fixtures.py
python3 scripts/run-actionlint.py --workflow-dir .github/workflows
```
