# Legacy Quickstart Hard Cutover

The public Linux install path is the release `install.sh`, not an artifact-only
`m80 quickstart --artifact-url` command. Users should install or repair with
one of these commands:

```sh
curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh
curl -fsSL https://github.com/moradology/m80/releases/download/<version>/install.sh | sudo sh
```

`m80 quickstart --artifact-url <url>` remains only for local fixture and
operator/test override flows. Public GitHub release artifact URLs must match the
running m80 binary. Dev builds and mismatched release binaries fail before
download with an exact pinned release `install.sh` command. Local `file://`
bundle URLs remain accepted as explicit local fixture overrides.

There is no compatibility matrix for stale artifact-only installs. The repair
path is to reinstall from the release bundle so the host binary, guest bundle,
metadata, verifier material, default profile, and host prerequisite checks move
together.

Regression coverage:

- `crates/m80-cli/src/cmds/quickstart.rs` unit tests pin dev-build and
  mismatched-release rejection for public release artifact URLs.
- `crates/m80-cli/tests/help_smoke.rs::help_quickstart` pins the help text to
  the override-only wording.
- `scripts/test-release-url-contract.py` rejects raw `main` installer URLs and
  artifact-only `releases/latest/download/*artifact*.tar.gz` snippets in public
  quickstart surfaces.
