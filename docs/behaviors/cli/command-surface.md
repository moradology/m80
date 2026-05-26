# CLI Command Surface

Behavior capture for bead `m80-lt15.1`.

## Contract

`m80` presents a constrained-process facade, not a VM lifecycle facade. The
headline command is:

```text
m80 run [OPTIONS] -- <program> [args...]
```

Top-level help must describe the tool as running a process with a constrained
view of the host. `m80 run` starts a sandbox, runs one process, mirrors stdout
and stderr in pipe mode, returns the guest exit code, and tears the sandbox down.

The requested program must exist in the selected guest runtime profile or in the
visible workspace. m80 does not execute host binaries, pull OCI images, or
install packages implicitly.

## Supported Commands

- `m80 run [OPTIONS] -- <program> [args...]`
- `m80 preflight`
- public Linux install: `curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh`
- pinned automation install: `curl -fsSL https://github.com/moradology/m80/releases/download/<version>/install.sh | sudo sh`
- release installer handoff: `m80 install --release-tag <tag>`
- `m80 quickstart --artifact-url <url> [--artifact-dir <path>] [--run-root <path>] [--no-run]`
- `m80 config show`
- `m80 list`
- `m80 inspect <vm-id>`
- `m80 metrics [--textfile <path>]`
- `m80 cleanup [--force]`
- `m80 net cleanup [--dry-run]`
- `m80 image build/list/show/rm/verify`
- `m80 template build/list/show/prune/rm`
- `m80 warm enable/status/drain/disable`
- `m80 version`

`m80 warm` is explicit warm-owner control. The implemented owner mode is a
visible foreground process:

```text
m80 warm enable --foreground --size <n> [--profile <name>] [--egress none|outbound]
m80 warm status [--profile <name>]
m80 warm drain
m80 warm disable
```

`m80 warm enable --system` remains a reserved feature gap until service
packaging lands. There is no hidden daemon started by `m80 run --warm`.

The landed image command contract is captured in
`docs/behaviors/cli/image-commands.md`.
The landed snapshot-template command contract is captured in
`docs/behaviors/cli/template-commands.md`. Its destructive prune path is scoped
to the selected BootSpec family so one store can safely hold multiple template
families.
The network orphan recovery command is captured in
`docs/behaviors/network/orphan-cleanup.md`.

The old VM-front-door commands are removed, not compatibility aliases:

- `m80 launch`
- `m80 exec`
- `m80 stop`
- `m80 snapshot`

## `m80 run` Flags

Implemented in v0.1:

- `--workspace <path>`
- `--cwd <path>`
- `--env KEY=VAL`
- `--secret-env KEY`
- `--stdin`
- `--profile <name>`
- `--egress none|outbound`
- `--scratch-size <bytes>`
- `--writeback never|on-success|always`
- `--warm`

The CLI egress default is `outbound`. Callers use `--egress none` for lockdown.
The lower library default may remain no-egress; this facade intentionally chooses
the process-wrapper default.

`--profile <name>` selects a local m80 runtime profile. If it is absent, the
effective `default_profile` config field is used. The built-in default is the
`env` profile, which resolves boot artifacts through the existing `M80_*`
preflight environment. Named profile behavior is captured in
`docs/behaviors/cli/image-profile-selection.md`.

`m80 preflight` uses that same selected profile for artifact and host-helper
checks. Its human output names the selected profile, active install pointer,
artifact/helper paths, release tag, and missing profile paths before the
preflight table; its JSON output wraps that same runtime-profile report beside
the `HostPrerequisiteResult`. Diagnostics for an installed default profile
therefore name the paths that plain `m80 run -- echo hello` will use.

Parsed but intentionally feature-gapped:

- `--allow-host <host>` / `--allow-cidr <cidr>` - egress allowlists.
- `--mount-config <host>:<guest>[:ro]` - config-file projection.
- `--keep-on-failure` - sandbox retention for diagnostics.

Feature-gapped flags exit 7 and explain the missing behavior. They must not be
silently ignored.

Implemented PTY flags:

- `--tty` / `-t` - allocate a guest terminal and stream the merged terminal
  bytes to host stdout.
- `-i` - forward live host stdin in PTY mode. It is invalid without `--tty`.

Implemented warm flag:

- `--warm` - lease a ready slot from the explicit foreground owner. It is
  invalid with `--workspace` and `--tty`, and it fails closed when the owner is
  unavailable or empty instead of cold booting.

## `m80 quickstart`

The normal Linux first-run path is the release `install.sh`, followed by plain
`m80 run -- echo hello`.
The public help order is stable/latest install first, pinned install second,
and explicit override surfaces last. `m80 install --help` and
`m80 quickstart --help` must both reject raw `main` URLs and artifact-only
latest examples.

`m80 quickstart --artifact-url <url>` is the explicit operator/test override for
local fixture tarballs or pinned artifact tarballs that match the running m80
binary. `scripts/quickstart.sh` is only a thin launcher for that override path.
Quickstart downloads the artifact tarball and the sibling `<url>.sha256`,
verifies the tarball before extraction, verifies the extracted `SHA256SUMS`,
installs `vmlinux`, `output.ext4`, `output.ext4.manifest.json`, and
`m80-guestd`, creates the run-root, writes the installed default profile/config,
and runs plain `m80 run -- echo hello` unless `--no-run` is set. Public GitHub
release tarball URLs are rejected when the binary is a dev build or a different
release. The probe intentionally uses the CLI default outbound egress policy,
matching the public follow-up command instead of proving only the no-egress
variant.
When the probe will run, quickstart checks the non-mutating host substrate
before changing active artifact paths.
The tarball must not contain `host-binaries.manifest.json`; that manifest is
generated from final host TCB paths before the probe run.
Global `--json` requires `--no-run` because the successful probe writes guest
stdout; the install-only JSON path emits a machine-readable artifact/profile
summary.
That JSON/install-only path is hostless: it does not claim host substrate
readiness or real-KVM launch proof.

Defaults:

- `--artifact-dir` defaults to `M80_ARTIFACT_DIR` or `/opt/m80/artifacts`.
- `--run-root` defaults to `M80_RUN_ROOT` or `/var/run/m80`.

The generated installed-default profile/config fields are captured in
`docs/behaviors/cli/installed-default-profile.md`.

The command is non-interactive and does not install packages, pull OCI images,
or infer a runtime. It consumes only an explicit artifact URL and is not the
common public install command.

## Output

Normal `m80 run` pipe mode is transparent and streaming:

- guest stdout -> host stdout
- guest stderr -> host stderr
- guest exit code -> process exit code

The command does not write VM IDs, run-root paths, or lifecycle status to stdout
in pipe mode. Diagnostics belong on stderr or in explicit diagnostic artifacts.
Output is copied as chunks arrive rather than after guest process exit; see
`docs/behaviors/cli/pipe-streaming.md`.

With global `--json`, `m80 run` emits the serialized exec response instead of
transparent pipe output. The response is wrapped in the versioned JSON envelope
described by `docs/behaviors/cli/json-output-envelope.md`.

`m80 metrics` is intentionally Prometheus text only. It reports run-root health
from disk, active warm-owner slot metrics when a warm owner is running, and
counters visible in the current process. Standalone CLI invocations do not
recover historical counters from prior `m80 run` processes; long-running
embedders use the `m80-firecracker` snapshot API and the `m80-observability`
renderer in-process. The command writes exposition text to stdout by default,
or atomically replaces the supplied `--textfile` path for node-exporter textfile
collection. The textfile parent directory must already exist; missing collector
directories fail closed instead of being created by m80. Global `--json` is
invalid for this command.

## Tests

The command contract is pinned by:

- `crates/m80-cli/tests/parse_args.rs`
- `crates/m80-cli/tests/quickstart_smoke.rs`
- `crates/m80-cli/tests/help_smoke.rs`
- `crates/m80-cli/tests/feature_gap_smoke.rs`

Those tests cover every supported command, the reserved flag shapes, global
`--json`, explicit feature-gap exits, and rejection of removed aliases.
