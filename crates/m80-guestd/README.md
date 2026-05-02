# `m80-guestd`

The in-VM daemon. Listens on vsock, accepts one connection per request,
runs argv with optional cwd/env, returns stdout/stderr/exit/timing.

## Reason for being

m80 puts a hardware-isolated VM around a single command. Something has
to be inside the VM listening for the command, running it, and reporting
back. That's `m80-guestd`.

The reframe relative to predecessor's `guestd-rs`: m80's guest daemon runs
**one operation** — exec argv. There is no fixed tool catalog (no
`bash`, no `read_file`, no `build`/`test` subcommand). Anything more
specialized — a tool catalog, idempotency dedupe, workspace policy
enforcement — is an adapter concern that can wrap m80, not bake into
its guest binary.

Keeping the guest small has direct benefits:

- The image is smaller (no Python/Node/etc. needed unless the embedder
  wants them).
- The attack surface is smaller (one operation, one envelope shape).
- The cross-compile target is simpler (statically-linkable, musl-friendly).

## Black-box contract

### Lifecycle

- Started by systemd at `multi-user.target` from the manifest-installed
  unit. `Type=simple Restart=on-failure`.
- On startup: bind vsock port (default **9001**), print `GUESTD_READY`
  to the serial console (the agreed ready marker), then loop on
  `accept()`.
- On each accepted connection:
  1. Read one `m80-proto::Envelope<ExecRequest>` (fail closed on
     version mismatch).
  2. Spawn the child process per the request (argv + optional cwd +
     optional env).
  3. Capture stdout/stderr to bounded buffers.
  4. Apply the request's `timeout_ms` budget; on expiry, SIGKILL.
  5. Reap, build `ExecResponse`, write it back as a `m80-proto`
     envelope.
  6. Sync filesystems (`sync(2)`) before close so post-stop change
     extraction sees the final state.
  7. Close.
- Concurrent connections per VM are **not supported in v0.1**. The
  daemon serializes (`accept()` returns one at a time, processes,
  closes, accepts again).
- On host disconnect mid-exec: kill the child immediately. Partial
  output may or may not have been flushed; the response is whatever
  state we observed.

### ExecRequest fields

- `argv: Vec<String>` — required. argv[0] must be an executable path.
- `cwd: Option<String>` — optional. Defaults to `/`. Must exist in the
  guest filesystem.
- `env: Option<Vec<(String, String)>>` — optional. Replaces (does not
  augment) the child environment when set.
- `stdin: Option<Vec<u8>>` — optional. Bytes piped to the child's stdin
  before close.
- `timeout_ms: Option<u64>` — optional. No timeout when unset; the
  daemon will run until the child exits.

### ExecResponse fields

- `status: ExecStatus { Completed, TimedOut, Cancelled, Failed }`.
- `exit_code: Option<i32>` — `None` if the process never produced one
  (e.g., killed).
- `stdout: Vec<u8>`, `stderr: Vec<u8>` — capped at a configurable per-
  stream limit (default 1 MiB; consider externalization in v0.2).
- `timing: { spawned_at, exited_at, spawn_ms, run_ms }`.

### Workspace mount

- The systemd-installed mount unit attaches the host-provided scratch
  ext4 (`/dev/vdb`) at the manifest-declared in-guest path before the
  daemon starts. The daemon does not mount anything itself.

## Public surface

This crate ships only a binary; no library API.

Command-line:
- `m80-guestd` — run with defaults baked into the manifest.
- `m80-guestd --port <u32>` — override the vsock port (testing only).
- `m80-guestd --version` — print the embedded protocol version + commit.

## Non-goals

- **No tool catalog.** No `bash`, `exec`, `read_file`, `write_file`,
  `build`, `test` subcommands. Wrap argv around `bash -c '...'` if
  you want shell semantics.
- **No workspace policy enforcement.** No `read_only` flag, no allowed-
  tools list. The host trusts the VM is running unprivileged code; the
  isolation boundary is the VM, not the daemon.
- **No persistent connection.** One connection = one exec.
- **No streaming output.** stdout/stderr are batched and returned at
  exit. Streaming is a v0.2 epic.
- **No cancellation envelope.** Cancellation is by host-side connection
  close.

## Dependencies

- `m80-proto` — wire envelope.
- `serde`, `serde_json`.
- `thiserror`, `anyhow`, `tracing`, `nix`.
- (Cross-compiled to the guest target. Runs on the kernel + rootfs that
  `m80-image-build` produced.)

## Tests

- Loopback: against a fixture vsock implementation, an `ExecRequest`
  with `argv=["true"]` returns `Completed { exit_code: 0 }`.
- Timeout: an `argv=["sleep","60"]` with `timeout_ms=100` returns
  `TimedOut` and the child is reaped within bounded time.
- Cancellation: closing the host connection mid-exec kills the child
  within bounded time and the partial output is observable in the
  log.
- Filesystem sync: a request that writes to the workspace, followed by
  a clean disconnect, leaves the scratch image in a state that
  post-stop `e2fsck` accepts without journal replay errors.
- Frame discipline: a malformed envelope closes the connection without
  affecting subsequent connections.
