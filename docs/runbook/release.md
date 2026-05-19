# Release Runbook

This runbook owns the release identity contract used by the installer and
quickstart flow.

## Version Source

Release builds inject the GitHub release tag at compile time:

```sh
M80_RELEASE_TAG=vX.Y.Z cargo build --release -p m80-cli
```

The injected tag must be the exact `v<workspace package version>` tag. For the
workspace package version `0.0.0`, the expected release tag is `v0.0.0`.

`m80 --version` and `m80 version` expose the release identity:

- dev builds render as `<package-version>-dev`;
- release builds render the injected GitHub release tag;
- `m80 --json version` includes `package_version`, `release_tag`,
  `release_build`, `version_status`, and `expected_release_tag`.

Packaging must refuse to publish a bundle when `version_status` is `dev` or
`mismatch`. The installer and quickstart resolver must not use `releases/latest`
from a dev build; dev builds require an explicit local bundle or artifact URL.

## Verification

The release identity is pinned by:

- `crates/m80-cli/src/release.rs` unit tests for dev, release, and mismatch
  identity;
- `crates/m80-cli/tests/version_smoke.rs` for `m80 --version` and JSON
  `m80 version` output.
