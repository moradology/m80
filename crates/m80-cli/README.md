# `m80-cli`

The `m80` binary. It is the process-wrapper facade over `m80-firecracker`:
run one command with a constrained view of the host, mirror stdout/stderr, return
the guest exit code, and remove the sandbox unless an explicit future retention
surface says otherwise.

## Reason for being

A thin parse-args -> call-library -> render-result layer. The contract worth
owning here is the user-facing command vocabulary, stable per-class exit codes,
and a `--json` mode for commands that should produce machine-readable output.
Every `--json` document is wrapped as
`{"version": 1, "request_id": "...", "data": ...}` when a run-scoped request id
exists, or `{"version": 1, "data": ...}` otherwise, so scripts can reject or
branch on breaking schema changes.

## Black-box contract

### Mental model

The CLI does not present VMs as the product. The front door is:

```text
m80 run [OPTIONS] -- <program> [args...]
```

`m80 run` starts a sandbox, runs exactly one process inside the selected guest
runtime, mirrors stdout to stdout and stderr to stderr in pipe mode, exits with
the guest process exit code, then tears the sandbox down.
The CLI mints one opaque ULID-based `request_id` at `m80 run` entry and threads
it through JSON envelopes, wrapper errors, host diagnostics, warm-owner control
requests, and guest wire frames.

Runtime availability is explicit: `<program>` must already exist in the selected
guest image/profile or in the visible workspace. The CLI does not execute host
binaries, pull OCI images, or install packages implicitly.

### Supported subcommands

- `m80 run [OPTIONS] -- <program> [args...]` - primary process-wrapper command.
- `m80 preflight` - resolves the effective config and selected runtime profile,
  then runs the same profile-aware host capability/artifact checks used before
  `m80 run`. Human output names the selected profile, active install pointer,
  artifact/helper paths, release tag, and missing profile paths before the
  preflight table. JSON output wraps that runtime-profile report plus the
  `HostPrerequisiteResult`. Exit 0 on full pass, 2 on any check failed.
- `m80 quickstart --artifact-url <url>` - explicit operator/test override for a
  local fixture tarball or a pinned artifact tarball that matches the running
  m80 binary. The normal Linux first-run path is the release `install.sh`.
  Quickstart downloads the tarball plus `<url>.sha256`, verifies the tarball
  before extraction, verifies the extracted `SHA256SUMS`, installs the
  kernel/rootfs/manifest/guestd artifacts, rejects bundled host-binaries
  manifests, checks the non-mutating host substrate before changing active
  artifacts when the probe will run, generates the installed default runtime
  profile/config, generates the installed host manifest from final host TCB
  paths before the probe, and runs plain `m80 run -- echo hello` unless
  `--no-run` is set. Public GitHub release tarball URLs are rejected when the
  binary is a dev build or a different release. The probe intentionally uses
  the CLI default outbound egress so it matches the public quickstart target and
  proves the default host prerequisite path. `--json` requires `--no-run` so
  guest probe stdout cannot pollute the machine-readable summary. `--no-run`
  does not prove host substrate readiness or real-KVM launch.
- `m80 install --release-tag <tag>|--bundle-url <url> [--dry-run]` - validates
  the release-bundle installer input contract. `--dry-run` prints the install
  plan without touching host state. `--release-tag` and the hidden
  `--bootstrap-tag` handoff fetch the checksum-verified asset index for the
  concrete release, select the Linux x86_64 minimal bundle by tag, OS,
  architecture, and image kind. Asset-index failures report stable text and
  JSON fields for the requested tuple/version, available alternatives, a
  machine-readable code, and a repair command when known. `--bundle-url`
  remains the explicit local fixture/operator override path. Concrete official
  GitHub release bundle URLs verify the same-tag public material set and bundle
  bytes before staging: asset index, checksum sidecars, installer, bootstrap
  selector, build manifest, release-integrity predicate subjects, normalized
  attestation metadata, the GitHub Artifact Attestation bundle, public
  `SHA256SUMS`, and the selected bundle digest. The attestation check binds the
  predicate to the moradology/m80 release workflow, tag ref, source commit, and
  trusted issuer before the selected bundle tarball is downloaded. Successful
  direct official URL installs name the resolved tag, bundle asset, installer
  digest, public checksum digest, asset-index digest, proof-cache destination,
  and whether the verified proof cache was written before activation. Failures
  include finite JSON `code` values for classifier, fetch, digest, attestation,
  stale-material, and no-write rollback cases plus one retry command when the
  operator should rerun the explicit URL.
  Non-dry-run stages, verifies, and copies the selected or explicit bundle into
  `<install-root>/versions/<release_tag>`, writes profile state, and switches
  `<install-root>/active` last.
- `m80 config show` - prints the merged effective config and labels each field's
  source.
- `m80 list` - enumerates VM run-dirs under the configured run-root, labeling
  each `live` or `stale`.
- `m80 inspect <vm-id>` - prints run-dir layout, recorded lifecycle state, boot
  identity, and available diagnostics for an existing run-dir.
- `m80 logs <vm-id> [--follow] [--request-id <id>] [--since <ts>]` - reads
  persisted out-of-band VM diagnostics from `<run_dir>/console.log` and
  `<run_dir>/diagnostics.jsonl`, interleaving guest and host records without
  replaying wrapped-process stdout/stderr.
- `m80 env` - prints the diagnostic dump for bug reports: host capabilities,
  effective config, selected runtime profile source, profile artifact/helper
  paths, active install pointer status, missing profile paths, Firecracker
  version, and run-root state. `m80 --json env` emits the same data in a
  versioned envelope.
- `m80 cleanup [--force]` - recovers stale run-root state and removes orphaned
  host resources where the lower crates expose cleanup.
- `m80 image build/gc/list/show/rm/verify` - manages content-addressed
  `m80-image-store` artifacts. The CLI can import a pre-built artifact file or
  build a small local-dev image from a directory, list and show records in human
  or JSON form, report or execute conservative GC, verify stored bytes, and
  refuse removal when active Shared refs or snapshot-template manifests still
  reference the digest.
- `m80 template build/list/show/prune/rm` - manages committed
  `m80-snapshot-template` artifacts. `build` parses a BootSpec and runs the
  Phase D snapshot-template producer path, `list`/`show` inspect committed
  manifests, `rm` removes one unpinned template, and `prune --boot-spec`
  removes only invalidated templates in the selected conservative family scope.
- `m80 warm` - explicit warm-sandbox owner control. Foreground owner mode is
  implemented with `enable --foreground`, `status`, `drain`, and `disable`;
  packaged system service mode remains reserved.
- `m80 version` - prints release identity, package version, protocol version,
  image artifact schema versions, and Firecracker pin. Dev builds render as
  `<package-version>-dev`; release packaging injects `M80_RELEASE_TAG` and
  `M80_RELEASE_COMMIT` plus `M80_RELEASE_TARGET_TRIPLE`, and the CLI exposes
  whether that tag matches the expected `v<package-version>` release plus the
  source commit and target identity bound to the release manifest.

Removed VM-front-door commands are not aliases: `launch`, out-of-process `exec`,
foreground `stop`, and `snapshot capture` are not accepted by the clap surface.
Reserved future surfaces are captured in
`docs/behaviors/cli/feature-gaps.md`.
Run-root list/inspect behavior is captured in
`docs/behaviors/cli/run-root-commands.md`.
The JSON envelope contract is captured in
`docs/behaviors/cli/json-output-envelope.md`.
Output and error behavior is captured in
`docs/behaviors/cli/output-and-errors.md`.
Image/profile selection is captured in
`docs/behaviors/cli/image-profile-selection.md`.
Installed default profile generation is captured in
`docs/behaviors/cli/installed-default-profile.md`.
CLI egress policy is captured in `docs/behaviors/cli/egress-policy.md`.
Warm lifetime semantics are captured in
`docs/behaviors/cli/warm-lifetime.md`.
Warm owner lifecycle/package semantics are captured in
`docs/behaviors/cli/warm-owner-lifecycle.md`.
Warm owner socket authorization and request bounds are captured in
`docs/behaviors/security/warm-owner-socket.md`.
Workspace visibility and process inputs are captured in
`docs/behaviors/cli/workspace-visibility.md` and
`docs/behaviors/cli/process-inputs.md`.
Scratch/rootfs effect limits are captured in
`docs/behaviors/cli/scratch-and-rootfs-effects.md`.
Workspace writeback policy is captured in
`docs/behaviors/cli/writeback-policy.md`.
Auth/config projection is captured in
`docs/behaviors/cli/auth-config-projection.md`.
The ignored real-KVM process-facade test is captured in
`docs/behaviors/cli/e2e-run-passthrough.md`.
Pipe-mode real-time output streaming is captured in
`docs/behaviors/cli/pipe-streaming.md`.
Signal and cancellation behavior is captured in
`docs/behaviors/cli/signal-cancellation.md`.
Interactive PTY behavior is captured in
`docs/behaviors/cli/interactive-pty.md`.
Request-id correlation is captured in
`docs/behaviors/cli/request-id-correlation.md`.
Out-of-band diagnostics tailing is captured in
`docs/behaviors/cli/diagnostic-logs.md`.
The diagnostic environment dump is captured in
`docs/behaviors/cli/env-dump.md`.
Image-store command behavior is captured in
`docs/behaviors/cli/image-commands.md`.
Snapshot-template command behavior is captured in
`docs/behaviors/cli/template-commands.md`.
Installer input behavior is captured in
`docs/behaviors/release/installer-input-contract.md`.
Release asset-index selection and diagnostics are captured in
`docs/behaviors/release/asset-index.md`.
Direct official URL diagnostics are captured in
`docs/behaviors/release/direct-url-diagnostics.md`.
Installed layout behavior is captured in
`docs/behaviors/release/installed-layout.md`.

### `m80 run` options

- `--profile <name>` - selects a local runtime profile for this run. The
  installed default profile points at the release-installed bundle. The
  built-in `env` profile remains available for the existing `M80_KERNEL_IMAGE`
  / `M80_ROOTFS_IMAGE` artifact discovery path. Named profiles are TOML files
  at `/etc/m80/profiles/<name>.toml` or
  `~/.config/m80/profiles/<name>.toml`.
- `--workspace <path>` - makes a host workspace visible to the guest.
- `--cwd <path>` - sets the guest process working directory.
- `--env KEY=VAL` - adds one guest environment override. May be repeated.
- `--secret-env KEY` - copies one explicitly named host environment variable
  into the guest environment. The key must exist on the host; nothing is
  inherited implicitly.
- `--stdin` - reads host stdin fully and sends it to the guest process.
- `--egress none|outbound` - selects egress policy. The CLI default is
  `outbound`; `none` disables guest egress.
- `--allow-host <host>` / `--allow-cidr <cidr>` - reserved allowlist shape.
  Until the egress allowlist behavior lands, using either exits 7.
- `--scratch-size <bytes>` - overrides scratch overlay size in bytes. Zero is
  rejected.
- `--overlay-clone-mode byte-copy|reflink|auto` - selects how the empty
  overlay template is cloned for the cold-booted VM. The default is
  `byte-copy`. `reflink` requires metadata-only CoW clone semantics. `auto`
  probes the run-root and selects one concrete mode before cloning; clone
  failures are not retried as another mode.
- `--vcpu-count <n>` - overrides the cold-booted VM vCPU count. Zero is
  rejected. This is incompatible with `--warm` because warm slot sizing is
  fixed by the owner.
- `--mem-size-mib <mib>` - overrides the cold-booted VM memory size in MiB.
  Zero is rejected. This is incompatible with `--warm` because warm slot sizing
  is fixed by the owner.
- `--writeback never|on-success|always` - controls workspace writeback.
  `never` is the default. `on-success` extracts workspace changes only after a
  zero guest exit; `always` extracts after zero or non-zero guest exits while
  preserving the guest exit code when extraction succeeds. Writeback requires
  `--workspace`.
- `--keep-on-failure` - reserved retention behavior; exits 7 until implemented.
- `--mount-config <host>:<guest>[:ro]` - reserved config-file projection shape;
  exits 7 until mount projection lands.
- `--tty` / `-t` - allocates a guest terminal and streams the merged terminal
  byte stream to host stdout. `-i` additionally forwards live host stdin after
  putting the host terminal in raw mode. The TUI shape is
  `m80 run -it --workspace . --egress outbound --secret-env ANTHROPIC_API_KEY -- claude`,
  assuming the selected guest profile already contains `claude`. `--json` and
  `--stdin` are invalid with `--tty`; `-i` is invalid without `--tty`.
- `--warm` - lease a clean, stateless, pre-restored slot from an explicit
  resident foreground owner. It never cold-boots as a hidden fallback. It is
  incompatible with `--workspace` until attach-late workspace support lands and
  incompatible with `--tty` until terminal lease ownership is designed.

### Warm lifetime status

Current foreground-owner shape:

```text
m80 warm enable --foreground --size <n> [--profile <name>] [--egress none|outbound]
m80 warm status [--profile <name>]
m80 warm drain
m80 warm disable
m80 run --warm -- <program> [args...]
```

A warm slot is restored from a clean snapshot, has passed the guestd ready
probe, has no user process running, and has no workspace/request/secret/terminal
attached. `m80 run --warm` fails if the resident owner is missing or empty; it
does not silently cold-boot. The foreground owner is a visible long-running
process with a Unix control socket under the warm run-root. Non-JSON warm runs
stream stdout/stderr frames over that control socket before the terminal exit
frame; JSON warm runs intentionally keep the buffered response shape. System
service packaging follows once that contract is proven; user-service ownership
is deferred.

### Image Store Commands

Current image-store shape:

```text
m80 image build <name> --source <path> --out <store> [--kind erofs|ext4]
m80 image gc [--store <store>] [--template-store <store>] [--keep <digest>...]
             [--pin-file <path>] [--min-age <duration>] [--execute]
m80 image list [--store <store>]
m80 image show <digest> [--store <store>]
m80 image verify <digest> [--store <store>]
m80 image rm <digest> [--store <store>] [--template-store <store>]
```

Images are content-addressed; `<name>` is an operator label echoed in command
output, not a persistent alias. A directory `--source` is built through the
small local-dev image-store builder, while a regular file is imported as a
pre-built artifact of the selected `--kind`. `list` and `show` render a tabular
human view by default and a versioned JSON envelope with `--json`.

`verify` re-hashes the stored bytes and validates the store metadata shape.
`rm` removes all artifact kinds for the digest only after the image store reports
zero active Shared refs and the selected snapshot-template store has no committed
manifest referencing that digest.

`gc` is dry-run by default. It reports candidate digests, protected digests,
protection reasons, and total reclaimable bytes. The mark set includes
committed template references, active Shared refs, repeated `--keep` digests,
newline-delimited `--pin-file` digests, and optional `--min-age` recency
retention. `--execute` is required to delete candidates and still uses the same
typed `ImageStore::remove` path as `rm`.

### Template Store Commands

Current snapshot-template store shape:

```text
m80 template build <name> --boot-spec <file>
m80 template list [--store <store>]
m80 template show <fingerprint> [--store <store>]
m80 template prune [--store <store>]
m80 template rm <fingerprint> [--store <store>]
```

The default template store is `/var/lib/m80/templates`. Template fingerprints
are content addresses derived from typed Phase D inputs; `<name>` is an operator
label echoed in build output, not a persistent alias.

`build` loads the BootSpec before preflight or VM admission. Syntax and typed
BootSpec errors therefore fail before any host action. For a valid
`snapshot_restore` BootSpec, the command uses the BootSpec's template-store
path and post-restore hook set, derives the real fingerprint from live host
inputs, and runs the snapshot-template producer path. The BootSpec's restore
fingerprint is a restore selector and is not treated as the build output
fingerprint.

`list` and `show` render human output by default and the standard versioned JSON
envelope with `--json`. `rm` removes only the requested unpinned template.
`prune` without `--boot-spec` opens the store and exits 0 with
`removed_count: 0`. `prune --boot-spec <file>` computes live inputs without
launching a VM and removes only unpinned templates with the same pmem image set,
post-init state digest, and hook set whose fingerprint no longer matches the
current host/kernel/Firecracker tuple.

### Output discipline

- In normal `m80 run` pipe mode, guest stdout is streamed to host stdout, guest
  stderr is streamed to host stderr, and the process exit code is the guest
  exit code. The CLI does not print VM IDs or lifecycle prose on stdout in this
  mode.
- `m80 logs` is a separate out-of-band diagnostics reader. It writes diagnostic
  records to its own stdout, but `m80 run` never uses that path to pollute the
  wrapped process stdout or stderr.
- `m80 run --warm` preserves the same pipe-mode stdout/stderr/exit behavior
  while leasing from the resident owner; no warm metadata is printed to stdout.
- During a running guest exec, SIGINT, SIGTERM, and SIGHUP cancel the guest
  child and then stop/delete the sandbox. A cancelled run exits with the
  conventional `128 + signal` status (`130` for SIGINT, `143` for SIGTERM).
- In PTY mode, terminal output is one merged byte stream rather than separated
  stdout/stderr, host terminal input is live only with `-i`, and global
  `--json` is invalid with `--tty`.
- With global `--json`, `m80 run` prints the serialized exec response rather
  than process-transparent pipe output, wrapped in the versioned JSON envelope.
- With global `--json`, every command that emits machine-readable output writes
  `{"version": 1, "data": ...}`. The integer version is monotonically
  increasing; breaking schema changes bump it.
- Errors render to stderr. Canonical mapping lives in
  `crates/m80-cli/src/errors.rs`. Summary:
  `1`=generic, `2`=preflight, `3`=admission, `4`=manifest, `5`=invalid state,
  `6`=config, `7`=explicit v0.x feature gap, `8`=warm pool empty.
- Clap parse/usage errors also exit `2` (`EX_USAGE`) before `m80-cli` dispatch
  runs. That numeric collision is intentional: parse failures are identified by
  clap-formatted stderr, while runtime preflight failures render through m80's
  plain or JSON error path.
- Stderr is informational until paired with a wrapper exit code or JSON error
  envelope. A successful guest command may write stderr; in pipe mode that is
  still guest stderr, not an m80 wrapper failure.

### Configuration sources

`m80-cli` honors the documented precedence chain:

1. Built-in defaults.
2. `/etc/m80/config.toml`.
3. `/etc/m80/config.d/*.toml` in lexicographic order.
4. `~/.config/m80/config.toml`.
5. `~/.config/m80/config.d/*.toml` in lexicographic order.
6. `M80_*` environment variables.
7. CLI flags.

`m80 config show` reveals the effective merged config and labels each field with
its source, including `default_profile` and whether file-derived values came
from a base config file or a config.d drop-in. Unknown config keys fail as
configuration errors instead of being ignored. The exact schema and precedence
contract are captured in
`docs/behaviors/configuration/env-schema.md` and
`docs/behaviors/configuration/loading-order.md`.

### Diagnostic environment

- `M80_PHASE_TRACE=1` enables stderr-only host lifecycle phase rows and forwards
  guest boot milestone rows used by the cold-launch benchmark and diagnostics
  docs. It is a diagnostic/debugging knob, not a stable script-facing output
  schema.

## Public surface

The binary itself. The lib target exposes only the parser and runner facade so
the binary and integration tests can exercise dispatch without subprocess-only
coverage; it is not a general embedding API.

Rust library items:

- `args` module - clap parser types.
- `runner` module - parsed-CLI dispatch.
- `Cli` - top-level clap parser.
- `Cmd` - supported subcommand enum.
- `ConfigAction` - `m80 config` action enum.
- `EgressMode` - `m80 run --egress` value enum.
- `InstallArgs` - `m80 install` argument struct.
- `OverlayCloneModeArg` - `m80 run --overlay-clone-mode` value enum.
- `QuickstartArgs` - `m80 quickstart` argument struct.
- `WarmAction` - `m80 warm` action enum.
- `WarmEnableArgs` - `m80 warm enable` argument struct.
- `WritebackMode` - `m80 run --writeback` value enum.

Stable surfaces:

- Subcommand names and arguments.
- `--json` output schemas.
- The JSON output envelope version.
- Exit codes per error class.
- Quickstart artifact tarball contents: `vmlinux`, `output.ext4`,
  `output.ext4.manifest.json`, `output.ext4.build-receipt.json`,
  `m80-guestd`, and `SHA256SUMS`. The tarball must not contain
  `host-binaries.manifest.json`.
- Installer source selection: `m80 install` accepts exactly one of
  `--release-tag`, `--bundle-url`, or the hidden bootstrapper handoff
  `--bootstrap-tag`. Dry-run planning does not write host state; indexed sources
  still fetch and verify the release asset index so the plan names the concrete
  bundle. Release-tag and bootstrap-tag sources select bundles through the
  checksum-verified release asset index. Non-dry-run layout copy accepts
  selected index URLs, local `file://` bundle URLs, local HTTP test fixtures,
  and concrete stable-tag moradology/m80 GitHub release bundle URLs matching
  `m80-<target>.tar.gz`. Mutable latest artifact URLs, raw branch URLs, foreign
  repositories, path-traversal asset paths, non-bundle release assets, and
  non-HTTPS GitHub release URLs are rejected before network access or
  install-root mutation. Concrete official GitHub release bundle URLs also
  preflight the same-tag public material set before staging so missing
  release-integrity, attestation, index, checksum, installer, selector, build,
  or public checksum assets fail before the bundle tarball is downloaded; the
  release-integrity predicate must also verify through `gh attestation verify`
  using the downloaded attestation bundle before selected bundle bytes are
  fetched.
  Accepted remote bundles are staged before publishing
  `<install-root>/versions/<release_tag>` and flipping the active pointer last.

## Non-goals

- No compatibility aliases for removed VM-lifecycle commands.
- No implicit package installation, OCI image pull, or host-binary execution.
- No daemon/remote-control API in `m80-cli`.
- No agent semantics: no `--tool`, no `--effect-class`, no semantic
  `workspace_id`, and no writeback authority policy.

## Dependencies

- `m80-firecracker` - orchestrator and exec response types.
- `m80-preflight` - for the `preflight` subcommand.
- `serde`, `serde_json` - JSON output.
- `toml` - runtime profile parsing.
- `thiserror`, `anyhow`, `tracing`.
- `clap` v4 - argument parsing.
- `nix`, `signal-hook`, `terminal_size` - terminal raw mode, signal, and size
  handling for PTY mode.

## Tests

- Argv parse: every documented invocation parses to the expected enum shape.
- Facade runner: dispatch is tested through `m80_cli::runner::run` without
  spawning the binary for every unit case.
- Config/preflight fixtures: isolated HOME/M80_* tests cover config precedence,
  selected installed-profile diagnostics, active-pointer status, and
  preflight's runtime-profile JSON wrapper, plus in-memory config/preflight
  render tests avoid KVM.
- Removed aliases: `launch`, `exec`, `stop`, and `snapshot` fail to parse.
- Help text: `--help` for the top level and every supported subcommand renders
  without error.
- Logs/env: parse/help tests cover the new CLI surfaces; module tests cover
  JSON envelope shape, request-id filtering, timestamp filtering, and bug-report
  dump sections without KVM.
- Quickstart: `m80 quickstart --no-run` is covered as an operator/test override
  with a local release-shaped tarball, external `.sha256` verification before
  extraction, extracted checksum verification, artifact install, run-root
  creation, and mismatch rejection.
- Install: `m80 install --dry-run` is covered for source exclusivity,
  install-root override, JSON output, and dev-build refusal for release-tag
  installs. Source selection is covered for checksum-verified asset-index
  success, missing defaults, duplicate defaults, wrong architecture, wrong
  image kind, wrong asset tag, bootstrap handoff, and explicit URL bypass.
  Bundle layout install is covered for local clean copy, HTTP fixture download,
  remote checksum mismatch, 404, truncated download cleanup, unsupported
  redirect cleanup, missing required bundle file, duplicate path, permission
  failure, dry-run no-write behavior, manifest/build-receipt relocation, and
  install provenance.
- Image/profile selection: local profile resolution, fail-closed profile
  parsing, and artifact env overlay are covered without KVM.
- Feature gaps: reserved `run` flags and `m80 warm enable --system` exit 7
  before backend work.
- PTY mode: parse/help, request construction, raw-mode restoration, resize
  forwarding, exit-code mapping, invalid JSON/stdin combinations, a
  noninteractive terminal smoke, and an ignored host-PTY interactive smoke are
  covered separately from pipe mode.
- Warm owner lifecycle: fixture tests pin unavailable-owner status,
  incompatible-profile status, drain/disable JSON transition fields, missing
  owner failure, no workspace warm fallback, empty-pool PoolEmpty behavior, and
  an ignored foreground-owner KVM smoke.
- Output/errors: wrapper failures keep stdout empty, render on stderr, and use
  stable exit codes; JSON wrapper failures render an enveloped error payload.
- Pipe streaming: normal `m80 run` uses the streaming exec path so early output
  is visible before process exit and large output is not capped by the buffered
  1 MiB response limit.
- Signal/cancellation: unit coverage pins signal exit mapping and an ignored
  real-KVM smoke sends SIGTERM to a long-running `m80 run`, expecting guest
  cancellation and sandbox deletion.
- Exit codes: each `FcError` variant exits with its documented code.
- End-to-end KVM tests live in the orchestration crate and in
  `crates/m80-cli/tests/e2e_run_passthrough.rs`; they remain ignored unless the
  host has the required Firecracker/KVM setup.
