# Legacy Quickstart Hard Cutover

The public Linux install path is the release `install.sh`. Artifact-only
`releases/latest` tarball downloads, raw `main` installer scripts, and
unqualified `m80 quickstart --artifact-url` instructions are out of the common
path.

Use one of these commands:

```sh
curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh
curl -fsSL https://github.com/moradology/m80/releases/download/<version>/install.sh | sudo sh
```

Use the first command for normal installs and repairs. Use the pinned form for
automation, rollbacks to an already chosen release, or incident repair where a
runbook names a tag.

There is no compatibility matrix for stale artifact-only installs. The repair
path is to reinstall from the release bundle so the host binary, guest bundle,
metadata, verifier material, default profile, and host prerequisite checks move
together.

| Situation | What to run | Why |
| --- | --- | --- |
| Pinned or dev binary used with a public artifact tarball | `curl -fsSL https://github.com/moradology/m80/releases/download/<version>/install.sh \| sudo sh` | Installs the matching host binary and guest artifacts from one release. |
| Existing artifact-only install | `curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh \| sudo sh` | Replaces the split install with a matched release bundle and generated host manifest. |
| Stale profile, stale active pointer, or stale proof cache | `m80 install-status`, then run the printed `next_action_command` | Keeps repair tied to the installed state m80 actually observed. |

`m80 quickstart --artifact-url <url>` remains only for local fixture and
operator/test override flows. Public GitHub release artifact URLs must match the
running m80 binary. Dev builds and mismatched release binaries fail before
download with an exact pinned release `install.sh` command. Local `file://`
bundle URLs remain accepted as explicit local fixture overrides.

<!-- m80:deprecated-quickstart start -->
Deprecated shapes, kept here only so the lint has one explicit migration-note
exception:

- `https://github.com/moradology/m80/releases/latest/download/m80-linux-x86_64-minimal-artifacts.tar.gz`
- `https://raw.githubusercontent.com/moradology/m80/main/scripts/install.sh`
<!-- m80:deprecated-quickstart end -->

Regression coverage:

- `crates/m80-cli/src/cmds/quickstart.rs` unit tests pin dev-build and
  mismatched-release rejection for public release artifact URLs.
- `crates/m80-cli/tests/help_smoke.rs::help_quickstart` pins the help text to
  the override-only wording.
- `scripts/test-release-url-contract.py` rejects raw `main` installer URLs and
  artifact-only `releases/latest/download/*artifact*.tar.gz` references outside
  this explicit deprecated migration-note block.
