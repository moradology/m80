# `m80-cli`

The `m80` binary. A thin command-line wrapper over `m80-firecracker`,
following the dossier's `09-cli-shape.md` design. Subcommands map 1:1 to
library calls so the CLI is essentially a translation layer between
shell args and Rust types.

## Reason for being

The dossier's three deliverables are:
1. A reusable Rust library.
2. A no-frills CLI for spawning, exec-ing into, and tearing down
   sandboxes.
3. The image-build pipeline.

`m80-cli` is deliverable #2. Keeping it thin (parse args → call
library → render result) means the CLI is automatically as
correct as the library; bugs aren't duplicated, and CLI changes don't
require library changes.

A second motivation: a real CLI with `--help`, `--json`, and `m80 config
show` is what makes m80 *self-evident*. A user who installs `m80`
without reading docs should be able to type `m80 --help` and get a
useful starting point.

## Black-box contract

### Subcommands

- `m80 preflight` — runs `m80-preflight::run()` and renders the table.
  Exit 0 on full pass, 2 on any check failed.
- `m80 launch [--workspace <path>] [--network noegress|outbound] [--id <vm-id>] [-- <argv>...]`
  — boots a VM in the foreground via `Sandbox::launch()`. Two modes:
  - **Blocking mode** (no trailing argv): boots and blocks indefinitely
    so the VM stays alive until SIGINT; the `Drop` chain on the
    `RunningSandbox` performs the cleanup.
  - **Single-shot mode** (`-- <argv>`): boots, sends one exec request,
    prints stdout/stderr/exit, then stops + deletes. v0.1's path for
    "spawn a sandbox to run one thing" — the separate `m80 exec`
    subcommand needs out-of-process IPC and is deferred to v0.2.
- `m80 exec <vm-id> -- <argv>... [--cwd <path>] [--env KEY=VAL ...]
  [--timeout-ms <n>]` — sends one exec request to a running VM. Renders
  stdout/stderr to the controlling terminal; exit code matches the
  guest's.
- `m80 stop <vm-id> [--extract-changes <dest>]` — stops the VM,
  optionally extracts changes via `m80-storage`, deletes the run-dir.
- `m80 inspect <vm-id>` — prints the run-dir layout, current lifecycle
  state, recorded boot identity, and (if available) recent diagnostics
  events.
- `m80 list` — enumerates VM run-dirs under the configured run-root,
  labeling each `live` (ownership.lock present + recorded pid alive in
  `/proc`) or `stale` (otherwise).
- `m80 cleanup [--force]` — runs `recover_stale_run_root()` plus
  `cleanup_orphan_bridge()` (if `m80-net-outbound` says so).
- `m80 config show` — prints the merged effective config in the
  documented precedence order.
- `m80 version` — prints binary version + protocol version + Firecracker
  pinned version.

### Output discipline

- Every subcommand has `--json` for machine-readable output. The shape
  is the corresponding `m80-firecracker` type, serialized via serde.
- Without `--json`, output is human-friendly: plain text with table
  rendering for `preflight` and `config show`. (No color in v0.1.)
- Errors render the typed `FcError` on stderr. Exit codes are stable
  per error variant (defined in `crates/m80-cli/src/errors.rs`):
  `1`=generic, `2`=preflight, `3`=admission, `4`=manifest, `5`=invalid
  state, `6`=config, `7`=v0.1 feature gap (e.g., the `m80 exec` stub).

### Configuration sources

`m80-cli` honors the documented precedence chain:

1. Built-in defaults.
2. `/etc/m80/config.toml`.
3. `~/.config/m80/config.toml`.
4. `M80_*` environment variables.
5. CLI flags (highest priority).

`m80 config show` reveals the effective merged config and labels each
field with its source.

## Public surface

The binary itself. No library API.

Stable surfaces:
- Subcommand names + arguments (semver-stable post-1.0).
- `--json` output schemas.
- Exit codes per `FcError` variant.

## Non-goals

- **No interactive shell.** `m80 exec` is one-shot; no REPL.
- **No daemon mode.** `m80 launch` runs in the foreground; for long-
  running services, embed `m80-firecracker` directly.
- **No remote control.** All operations are local-host. SSH/network
  remoting is a wrapper concern.
- **No agent semantics.** No `--tool`, no `--effect-class`, no
  workspace policy flag. Adapters wrap `m80` for that.

## Dependencies

- `m80-firecracker` — the orchestrator.
- `m80-preflight` — for the `preflight` subcommand.
- `serde`, `serde_json` — `--json` output.
- `thiserror`, `anyhow`, `tracing`.
- A small CLI-arg crate (e.g., `clap` v4) — to be picked at v0.1 impl
  time. Not declared yet to avoid premature commitment.

## Tests

- Argv parse: every documented invocation parses to the expected
  internal call.
- `--json` schemas: snapshot-tested per subcommand.
- Exit codes: each `FcError` variant exits with its documented code.
- Help text: `--help` for every subcommand renders without error
  (smoke test against `cargo run -- ... --help`).
- End-to-end (KVM-required, ignored by default): `m80 preflight && m80
  launch ... && m80 exec ... && m80 stop` against a real Firecracker.
