# Release Bundle Runbook

This runbook records the release workflow authority boundary for the Linux
bundle path.

## Workflow Stages

`CI` runs `python3 scripts/lint-github-workflows.py` on every push and pull
request. The linter rejects broad workflow permissions, release workflows
without a concurrency group, floating third-party action refs, and
`pull_request` workflows that reference `secrets.*`.

`Release artifacts` uses a concurrency group keyed by the GitHub ref name:
`release-artifacts-${{ github.ref_name }}`. For tag pushes, that key is the
release tag. For manual dispatches, it is the selected ref.

`build-release-artifacts` has `contents: read`. It checks out the repository,
installs the pinned Rust toolchain from this repo's toolchain policy, builds
the release artifacts, packages the tarball/checksum pair, and uploads only a
workflow artifact.

`publish-release-artifacts` runs only for tag refs after the build job
finishes. It is the only job with `contents: write`. It downloads the workflow
artifact and uploads the exact files to the matching GitHub Release with
`gh release upload`.

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
