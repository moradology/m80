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

- Started either by systemd at `basic.target` (ubuntu image kind,
  manifest-installed unit, `Type=simple Restart=on-failure`,
  `StandardOutput=journal+console`, `StandardError=journal+console`) or by the
  kernel as PID 1 (minimal image kind, `init=/m80-guestd` boot arg).
- **PID-1 mode** is detected at startup (`getpid() == 1`). When active:
  duplicate stdout and stderr to `/dev/console`, install a panic hook
  that exits non-zero, execute the overlay+pivot startup sequence (see
  below), and poll-reap orphaned children between vsock requests so
  re-parented orphans don't accumulate. No SIGCHLD or SIGTERM handlers
  — the workspace forbids `unsafe` and the firecracker host stops the
  VM with SIGKILL on the outside.
- On startup: bind vsock port (default `m80_proto::GUEST_PORT_DEFAULT`),
  emit a structured ready log containing `m80_proto::READY_MARKER_DEFAULT`,
  connect back to the host ready port, then loop on `accept()`.
- On each accepted connection:
  1. Read one
     `m80-proto::Envelope<ExecRequest | PtyRequest | file-op | MetricsRequest | ShutdownRequest>`
     (fail closed on version mismatch).
  2. For `ExecRequest` / `PtyRequest`, spawn the child process per the request
     (argv + optional cwd + optional env).
  3. If `ExecRequest::streaming == false`, capture stdout/stderr to
     per-stream 1 MiB buffers; if either cap is hit, the response's
     `truncated` field is set to `Some(true)`.
  4. If `ExecRequest::streaming == true`, stream stdout/stderr as
     bounded chunks (`ExecStdout` / `ExecStderr`) and finish with one
     `ExecExit` terminal frame.
  5. If the request is `PtyRequest`, allocate a pseudo-terminal, spawn the
     requested command as the foreground terminal process, bridge
     `PtyInput`/`PtyOutput`, apply `PtyResize`, honor `PtyControl`, and finish
     with one `PtyExit` terminal frame.
  6. If the request is a file-op verb, run it directly in guestd:
     read/write/list/stat/remove a path, or run the chunked upload
     begin/chunk/commit sequence on the same connection.
  7. If the request is `MetricsRequest`, read `/proc/stat` and
     `/proc/meminfo`, attach guestd-local request/error counters, and return
     `MetricsResponse`.
  8. For exec / PTY, apply the request's `timeout_ms` budget; on expiry,
     terminate the child process group.
  9. For exec / PTY, reap, build the terminal response, and write it back as
     one or more `m80-proto` envelopes. Direct file-op and metrics requests
     write their direct response without spawning a child.
  10. Exec, PTY, file-op, and shutdown paths sync filesystems before close so
     post-stop change extraction sees the final state. Metrics is read-only and
     does not force a filesystem sync.
  11. Close.
- Concurrent connections per VM are **not supported in v0.1**. The
  daemon serializes (`accept()` returns one at a time, processes,
  closes, accepts again).
- On host disconnect mid-exec: terminate the child process group immediately.
  Partial output may or may not have been flushed; the response is whatever
  state we observed.
- On `cancel_request` for the in-flight request id: terminate the child process
  group, reap the direct child, write `CancelResponse`, and do not write a terminal
  `ExecExit` for that cancelled request. A mismatched or late cancel returns
  `AlreadyExited` and the normal exec result continues. The in-flight PID slot
  is recovered through mutex poisoning because it stores only an optional child
  pid; poisoning must not crash guestd.

Exec children are started in a fresh process group. Cancel, timeout,
disconnect/read EOF, and streaming write failure use SIGTERM, wait a bounded
100 ms, then use SIGKILL against the same process group. This prevents
shell-spawned descendants from keeping stdout/stderr open after the wrapper has
cancelled the run.

### ExecRequest fields

(These are the fields on `m80_proto::ExecRequest`; the daemon deserializes
that wire type directly.)

- `program: String` — required. The executable path.
- `args: Vec<String>` — arguments passed to `program` (not including
  `program` itself).
- `cwd: Option<String>` — optional. Defaults to `/`. Must exist in the
  guest filesystem.
- `env: Option<Vec<(String, String)>>` — optional. Replaces (does not
  augment) the child environment when set.
- `stdin: Option<Vec<u8>>` — optional. Bytes piped to the child's stdin
  before close. The daemon rejects stdin larger than 1 MiB before spawning the
  child.
- `timeout_ms: Option<u64>` — optional. When unset or above the daemon
  ceiling, the daemon applies its one-hour maximum.
- `streaming: bool` — optional on the wire, defaults false. `false`
  returns one buffered `ExecResponse`; `true` emits zero or more
  `ExecStdout` / `ExecStderr` envelopes followed by one `ExecExit`.

### ExecResponse fields

- `status: ExecStatus { Completed, TimedOut, Cancelled, Failed }`.
- `exit_code: Option<i32>` — `None` if the process never produced one
  (e.g., killed).
- `stdout: Vec<u8>`, `stderr: Vec<u8>` — capped at a configurable per-
  stream limit (default 1 MiB; consider externalization in v0.2).
- `timing: { spawned_at, exited_at, spawn_ms, run_ms }`.

### Streaming exec fields

When `ExecRequest::streaming == true`, stdout/stderr are emitted as
`ExecStdout { seq, bytes }` and `ExecStderr { seq, bytes }`. Sequence
numbers are monotonic per stream. The terminal frame is
`ExecExit { status, exit_code, total_stdout_bytes, total_stderr_bytes,
truncated, timing }`; it is written after both capture threads drain.

The stream uses a one-frame bounded handoff from capture threads to the
connection writer. A slow host therefore backpressures the child through
the guest pipe instead of growing an unbounded guest buffer.

Behavior details:

- `docs/behaviors/exec/streaming-frame-order.md`
- `docs/behaviors/exec/streaming-cancellation.md`
- `docs/behaviors/exec/streaming-backpressure.md`

### PTY exec fields

PTY exec is a separate wire mode from pipe exec. The daemon receives
`PtyRequest { program, args, cwd, env, timeout_ms, size }`, opens a
pseudo-terminal through `portable-pty`, and spawns the requested program as the
foreground terminal process. `env` and `cwd` follow pipe-mode semantics:
`env: Some(_)` replaces the child environment; `env: None` inherits guestd's
current environment; `cwd: None` inherits guestd's current working directory.

While the child is running:

- host `PtyInput { seq, bytes }` frames are written to the PTY master
- guest PTY output is emitted as `PtyOutput { seq, bytes }`
- host `PtyResize { seq, size }` frames update the kernel PTY size
- host `PtyControl::Eof` drops the PTY writer
- host `PtyControl::Signal { signal }` sends the requested signal to the
  child process group

The terminal result is exactly one
`PtyExit { status, exit_code, exit_signal, total_input_bytes,
total_output_bytes, truncated, timing }` frame after PTY output drains. PTY
output is merged terminal output; it is not split into stdout and stderr.

Timeout, `cancel_request`, host disconnect/read EOF, and output write failure
terminate the PTY child process group with the same SIGTERM/100 ms/SIGKILL
policy used by pipe streaming. Cancellation writes `CancelResponse` and does not
write `PtyExit` for that request.

The PTY output reader is an internal helper thread. If it panics, guestd logs
the failure and ends the PTY session path; the panic is not propagated into the
connection loop.

Behavior details:

- `docs/behaviors/wire-protocol/pty.md`
- `docs/behaviors/cli/interactive-pty.md`

### Metrics fields

Metrics are direct guestd handlers, not shell commands. `MetricsRequest {}` is
served on demand from procfs and guestd-local counters. `MetricsResponse`
contains fixed-shape CPU tick counters, memory byte gauges, and
`requests_total` / `errors_total`. See
`docs/behaviors/observability/guest-metrics-vsock.md`.

### File-op fields

File operations are direct guestd handlers, not shell commands. Dispatch table:

| Request kind | Response kind | Behavior |
|---|---|---|
| `file_read_request` | `file_read_chunk` stream | Open final component with `O_NOFOLLOW`, read up to `max_bytes` or `FILE_READ_LIMIT_DEFAULT`, emit bounded chunks, and finish with one terminal `done` chunk that reports `truncated` or `error`. |
| `file_write_request` | `file_write_response` | Create/truncate one file with final-component `O_NOFOLLOW`; parent directory must exist; optional mode is applied after write. |
| `file_list_request` | `file_list_response` | List one directory level with `DirEntry { name, kind, size }`; no recursion. |
| `file_stat_request` | `file_stat_response` | `symlink_metadata` one path and return kind, size, mtime, mode. |
| `file_remove_request` | `file_remove_response` | Remove one non-directory path; directories return `IsADirectory`. |
| `file_write_begin/chunk/commit` | matching responses | Per-connection upload table writes `<path>.m80-upload.<upload_id>`, requires zero-based monotonic chunk sequences, acks chunks, fsyncs, then renames on commit. Disconnect drops the table and removes temp files. |

Errors are returned as `FileError` variants on the response, including
`InvalidSequence` for malformed chunked uploads. m80-guestd does not enforce
path-prefix policy, chown/permissions verbs, recursive listing, or adapter-level
tool semantics.

Behavior details:

- `docs/design/wire-fops.md`
- `docs/behaviors/wire-protocol/file-read.md`
- `docs/behaviors/wire-protocol/file-write.md`
- `docs/behaviors/wire-protocol/file-list.md`
- `docs/behaviors/wire-protocol/file-stat.md`
- `docs/behaviors/wire-protocol/file-remove.md`
- `docs/behaviors/wire-protocol/chunked-upload.md`

### PID-1 overlay+pivot startup sequence

Implements `docs/design/storage-overlay.md §3.1` (11-step pseudocode).
Executed in order during `enter_pid_one_mode()` before the vsock listener binds:

1. Mount pseudo-filesystems: `/proc` (procfs), `/sys` (sysfs), `/dev` (devtmpfs). `EBUSY` (kernel pre-mounted) is accepted as success.
2. Make mount namespace fully private (`MS_REC | MS_PRIVATE` on `/`) so `pivot_root(2)` does not propagate to the host.
3. Verify the image-built `/lower` mountpoint exists, then mount `/dev/vda` (shared read-only base ext4) there (`MS_RDONLY`).
4. Verify the image-built `/upper` mountpoint exists, then mount `/dev/vdb` (per-VM writable overlay ext4) there.
5. `mkdir /upper/root` and `mkdir /upper/.work` (idempotent — first boot creates, later boots already have them from a prior VM that used the overlay).
6. Verify the image-built `/merged` mountpoint exists.
7. Mount overlayfs: `lowerdir=/lower,upperdir=/upper/root,workdir=/upper/.work` at `/merged`.
8. Bind-mount `/proc` (`MS_BIND|MS_REC`), `/sys` (`MS_BIND`), `/dev` (`MS_BIND`) into `/merged/{proc,sys,dev}` so they survive pivot.
9. Apply `MS_SLAVE|MS_REC` on `/` and `MS_BIND|MS_REC` of `/merged` onto itself (required by `pivot_root(".", ".")`).
10. Call `pivot_rootfs("/merged")` — lifted verbatim from `kata-containers/src/agent/rustjail/src/mount.rs:523-559` (Apache-2.0, © 2019 Ant Financial). Uses `defer!` (scopeguard) for FD cleanup.
11. Mount `/dev/vdc` (workspace scratch ext4) at `/workspace` **inside the pivoted root**. Skipped if `/dev/vdc` does not exist (workspace is optional).

**Failure policy:** any step failure panics. No retry, no fallback —
failure here is structural. Every step logs to stderr in the structured
guest-log format below so the Firecracker serial console shows the exact
failure point.

The base root is already mounted read-only when PID 1 starts, so
`/lower`, `/upper`, and `/merged` are part of the minimal image-build
contract. `m80-guestd` checks them rather than creating them at boot.

### Workspace mount

- **Ubuntu image**: the systemd-installed mount unit attaches the host-
  provided scratch ext4 at `/workspace` before the daemon starts. The
  daemon does not mount anything itself.
- **Minimal image (PID-1 mode)**: m80-guestd mounts `/dev/vdc` →
  `/workspace` itself (step 11 above) after `pivot_root`, inside the
  merged overlayfs root. If `/dev/vdc` does not exist (Sandbox launched
  without a workspace directory), the mount is skipped — workspace is
  documented-optional, not an error. **Note:** before the overlay pivot,
  the workspace drive was `/dev/vdb`; after the overlay pivot (`m80-ovrl.4`),
  it is `/dev/vdc` (drive position 3 per `docs/design/storage-overlay.md §2`).

### Guest stderr format

All internal guestd lifecycle logs go to stderr with this line shape:

```text
[<RFC3339-timestamp>] [<phase>] [<request_id-or-boot>] <level> <message>
```

`phase` is one of `Boot`, `Ready`, `Exec`, or `Shutdown`.
`request_id` is the opaque request id from the host when one exists, and
`boot` before a request is in scope. `level` is `ERROR`, `WARN`,
`INFO`, or `DEBUG`. Log emission is best-effort and never changes
control flow.

In systemd images, the unit routes stdout and stderr to
`journal+console`; in PID-1 images, guestd duplicates stdout/stderr to
`/dev/console` before emitting startup logs. m80-firecracker captures
the resulting Firecracker stdout/stderr stream into `<run_dir>/console.log`.

PID-1 images also emit machine-readable boot milestone lines:

```text
M80_GUEST_BOOT name=<milestone> elapsed_us=<micros> delta_us=<micros>
```

`elapsed_us` is monotonic time since guestd process start. `delta_us` is
time since the previous milestone. The CLI forwards these lines to stderr
only when `M80_PHASE_TRACE=1`, so the cold-launch bench can place guest
milestones next to host-side `phase_12b_ready_accept` without changing
normal command output.

## Public surface

Binary-only; no library API. `m80-guestd --help` for flags.

## Non-goals

- **No tool catalog.** No `bash`, `exec`, `read_file`, `write_file`,
  `build`, `test` subcommands. Wrap argv around `bash -c '...'` if
  you want shell semantics.
- **No workspace policy enforcement.** No `read_only` flag, no allowed-
  tools list. The host trusts the VM is running unprivileged code; the
  isolation boundary is the VM, not the daemon.
- **No persistent connection.** One connection = one exec or PTY session.
- **No request multiplexing.** One connection carries one exec request
  and that request's response stream.

## Dependencies

- `m80-proto` — wire envelope.
- `vsock` — Linux `AF_VSOCK` listener.
- `thiserror`, `anyhow`, `tracing`, `nix`.
- `scopeguard` — `defer!` macro used in `pivot_rootfs` for FD cleanup.
- (Cross-compiled to the guest target. Runs on the kernel + rootfs that
  `m80-image-build` produced.)

## Tests

- Loopback: against a fixture vsock implementation, an `ExecRequest`
  with `program="true"` returns `Completed { exit_code: 0 }`.
- Timeout: an `ExecRequest` with `program="sleep", args=["60"]` and
  `timeout_ms=100` returns
  `TimedOut` and the child is reaped within bounded time.
- Cancellation: closing the host connection mid-exec kills the child
  within bounded time and the partial output is observable in the
  log.
- Filesystem sync: a request that writes to the workspace, followed by
  a clean disconnect, leaves the scratch image in a state that
  post-stop `e2fsck` accepts without journal replay errors.
- Frame discipline: a malformed envelope closes the connection without
  affecting subsequent connections.
