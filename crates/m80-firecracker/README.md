# `m80-firecracker`

The orchestrator. Composes the foundation crates into a usable sandbox:
preflight → boot → ready → run → stop → cleanup. Owns the lifecycle
state machine, the run-root layout, the admission semaphore, and the
configuration loading order.

## Reason for being

The foundation crates are leaves with crisp contracts; this is the one
place they compose. Without it every consumer of m80 would assemble the
lifecycle by hand — exactly what makes Firecracker hard to use out of
the box.

## Black-box contract

### Lifecycle state machine

```
Created → Running → Stopped → (Deleted | preserved-for-triage)
```

Each state is a distinct Rust type (`Sandbox`, `RunningSandbox`,
`StoppedSandbox`); transitions consume the prior handle so callers can't
double-stop or exec on a stopped VM. `force_kill` collapses Running →
Stopped while preserving the run-dir for offline inspection.

The Created → Running transition runs the strict 12-phase preboot pipeline
internally (via `Sandbox::launch`). Phases are sub-steps, not states —
failures at any phase return a typed `FcError` and drop the admission permit.
When `SandboxConfig::request_id` is set, launch, request, stop, and delete
events carry that opaque id in `<run_dir>/diagnostics.jsonl`.

`Sandbox::launch_from_snapshot` is an alternative Created → Running
transition for the warm-pool restore path. It skips the full cold-boot
pipeline and instead loads a snapshot pair into a new Firecracker process.
The VM is left Running after a successful restore; the exec channel readiness
is confirmed by probing `CONNECT 9001` against the restored vsock UDS
(retry loop, 50 ms sleep, 5 s cap).

`RunningSandbox::capture` pauses the live VM and writes a Full snapshot pair
to caller-supplied paths. The VM is left in the Paused state after a
successful call; the caller must then `stop()` or resume the VM. Snapshot
paths are host paths. `m80-firecracker` bind-mounts their parent directory
into the jail at `/snapshot` before calling Firecracker, because jailed
Firecracker cannot see arbitrary host paths outside the chroot.

`RunningSandbox::exec(&mut self, ...)` supports sequential multi-exec on the
same VM. Each call opens a fresh vsock connection to guestd, performs exactly
one request/response exchange, and closes that connection; the VM and its
writable overlay remain alive for the next call.

`RunningSandbox::exec_streaming(&mut self, ..., on_chunk)` is the real-time
stdout/stderr form. It forces `ExecRequest::streaming = true`, invokes
`on_chunk` for each `ExecChunk::Stdout` / `ExecChunk::Stderr` frame, and
returns the terminal `ExecExit`. The callback is fallible; returning `Err`
closes the streaming connection, which guestd treats as cancellation for the
in-flight child. The buffered `exec` method is built on top of this path and
preserves the existing 1 MiB per-stream cap. If the streaming caller unwinds or
otherwise drops the channel mid-request, guestd treats the disconnect as
cancellation and reaps the child before accepting the next exec.

If Firecracker's restored-vsock local-init path transiently fails during
`CONNECT`, or accepts `CONNECT` but the request-frame write fails with
`BrokenPipe`, `exec` retries the open+send step for a fixed 2.5 s budget.
Receive-side failures are not retried, because the guest may already have
executed the request.

Every exec and PTY request sends an opaque `Envelope::request_id` to guestd.
When the caller set `SandboxConfig::request_id`, that same id is used; otherwise
`m80-firecracker` generates a per-request id from the VM id and monotonic time.
Guestd echoes that id on response, streaming, PTY, and cancel frames and stamps
guest stderr with it.

`RunningSandbox::exec_with_cancel` and
`RunningSandbox::exec_streaming_with_cancel` add host-requested cancellation to
the same exec paths. They attach an opaque request id, clone a write-only
sender for the same vsock connection, and send `cancel_request` when the caller
signals the provided receiver. `CancelAck { Cancelled }` returns
`ExecStatus::Cancelled`; `CancelAck { AlreadyExited }` keeps waiting for the
normal terminal exec result.

`RunningSandbox::exec_pty(&mut self, PtyRequest, Receiver<PtyHostEvent>,
on_output)` is the interactive terminal form. It opens one vsock connection,
forwards `PtyHostEvent::Input`, `Resize`, `Control`, and `Cancel` frames on a
cloned sender for that same connection, invokes `on_output` for each merged
terminal `PtyOutput` frame, and returns the single terminal `PtyExit`. PTY
mode does not expose separated stdout/stderr streams.

`RunningSandbox` also exposes direct file-operation wrappers:
`read_file`, `write_file`, `list_dir`, `stat_file`, `remove_file`, and
`upload_file_chunked`. These construct m80-proto file-op envelopes and map
guest `FileError` responses to `FcError::FileOp`; callers do not need to
spawn `bash -c`, base64 data through stdout/stderr, or build envelopes by hand.

### Public lifecycle methods

| Method | Signature | Description |
|---|---|---|
| `RunningSandbox::exec` | `(&mut self, ExecRequest) -> Result<ExecResponse, FcError>` | Run one command and return buffered stdout/stderr/exit. |
| `RunningSandbox::exec_with_cancel` | `(&mut self, ExecRequest, std::sync::mpsc::Receiver<()>) -> Result<ExecResponse, FcError>` | Run one buffered command and send guest cancellation when the receiver fires. |
| `RunningSandbox::exec_streaming` | `(&mut self, ExecRequest, impl FnMut(ExecChunk) -> Result<(), FcError>) -> Result<ExecExit, FcError>` | Run one command and deliver stdout/stderr chunks before terminal exit. |
| `RunningSandbox::exec_streaming_with_cancel` | `(&mut self, ExecRequest, std::sync::mpsc::Receiver<()>, impl FnMut(ExecChunk) -> Result<(), FcError>) -> Result<ExecExit, FcError>` | Streaming exec plus same-connection guest cancellation. |
| `RunningSandbox::exec_pty` | `(&mut self, PtyRequest, std::sync::mpsc::Receiver<PtyHostEvent>, impl FnMut(PtyOutputChunk) -> Result<(), FcError>) -> Result<PtyExit, FcError>` | Run one terminal command and deliver merged PTY output before terminal exit. |
| `RunningSandbox::read_file` | `(&mut self, path, max_bytes) -> Result<(Vec<u8>, bool), FcError>` | Read bytes directly from the guest and report truncation. |
| `RunningSandbox::write_file` | `(&mut self, path, bytes, mode) -> Result<u64, FcError>` | Write one guest file directly. |
| `RunningSandbox::list_dir` | `(&mut self, path) -> Result<Vec<DirEntry>, FcError>` | List one guest directory level. |
| `RunningSandbox::stat_file` | `(&mut self, path) -> Result<FileStat, FcError>` | Stat one guest path without following final symlink. |
| `RunningSandbox::remove_file` | `(&mut self, path) -> Result<(), FcError>` | Remove one non-directory guest path. |
| `RunningSandbox::upload_file_chunked` | `(&mut self, path, mode, reader, chunk_size) -> Result<u64, FcError>` | Upload via begin/chunk/commit on one vsock connection. |
| `Sandbox::launch_from_snapshot` | `(self, snapshot: SnapshotPaths, discovery: &Discovery) -> Result<RunningSandbox, FcError>` | Restore a snapshot into a new Running sandbox. |
| `RunningSandbox::capture` | `(&mut self, paths: SnapshotPaths) -> Result<(), FcError>` | Capture the live VM; leaves VM Paused and records snapshot-capture stop evidence. |

`SnapshotPaths` is re-exported from `m80-snapshot` for caller convenience.
`SandboxConfig::request_id` is optional and opaque; it is for diagnostics and
wire-frame pairing only, not an agent semantic identifier.

### First-line machine shape

`FIRST_LINE_VCPU_COUNT` and `FIRST_LINE_MEM_SIZE_MIB` name the default
Firecracker shape: 1 vCPU and 1024 MiB. Omitted `SandboxConfig::vcpu_count`
and `SandboxConfig::mem_size_mib` resolve to those values during preboot.
Callers may still supply explicit sizing for ordinary launches. Snapshot
restore and warm-pool timing fixtures use the exported constants so latency
proofs do not drift to a benchmark-only smaller VM.

### Cleanup contract vocabulary

`m80-firecracker` exports small enums/constants that pin the cleanup contract
for docs and regression tests:

| Item | Description |
|---|---|
| `CleanupPhase` / `CLEANUP_PHASE_ORDER` | `AdmissionFence -> BoundedStop -> OptionalChangeExtract -> ResidueCleanup -> Release`. |
| `StopDisposition` / `STOP_DISPOSITIONS` | Normal `GuestdShutdownThenFirecrackerKill` and explicit `HostForceKill`. |
| `CleanupReleaseBlocker` / `CLEANUP_RELEASE_BLOCKERS` | Generic VM-mechanics blockers: ambiguous force kill, cleanup failure, and possibly-live owned residue. |
| `CleanupAuthority` / `CLEANUP_AUTHORITY` | Documents that m80 emits evidence only and does not advance placement state. |

These names do not add agent semantics. They are behavior vocabulary for the
generic VM cleanup surface.

### Warm pool

`WarmPool` is the clean-VM latency path built on top of
`Sandbox::launch_from_snapshot`. It owns pre-restored, guestd-ready
`RunningSandbox` slots and leases one slot at a time through `WarmLease`.

Public surface:

| Type / method | Description |
|---|---|
| `WarmPoolConfig` | Target ready-slot count, snapshot pair, stateless `SandboxConfig`, ready-probe `ExecRequest`, and VM id prefix. |
| `WarmPool::new` | Constructs the pool; rejects `target_ready == 0` and workspace-backed configs. |
| `WarmPool::fill_to_target_blocking` | Synchronously pre-restores ready slots before serving traffic. |
| `WarmPool::try_lease` | Returns a ready `WarmLease` or `FcError::PoolEmpty`; never cold-boots or restores on the allocation path. |
| `WarmLease::exec` | Delegates one exec to the leased `RunningSandbox`. |
| `WarmLease::exec_with_request_id` | Delegates one exec while temporarily stamping the leased slot with the caller request id. |
| `WarmLease::exec_streaming` | Delegates one streaming exec to the leased `RunningSandbox`, preserving stdout/stderr chunks before terminal exit. |
| `WarmLease::exec_streaming_with_request_id` | Streaming exec plus temporary caller request-id stamping. |
| `WarmLease::discard` | Kills and deletes the slot, then starts background refill. `Drop` performs the same discard best-effort. |
| `BlankVmResetEvidence` | Eight explicit evidence inputs required before a blank VM can ever re-enter `Ready`. |

The first implementation never infers reuse from liveness, process
handles, socket existence, metrics, or clean-looking directories.
Without complete `BlankVmResetEvidence`, leases are discarded and
replaced. A restored slot does not enter `Ready` until its configured
ready-probe exec completes successfully. See `docs/design/warm-pool.md`
for the state machine and sizing model.

### Run-root layout

All per-VM state lives under `<run_root>/<vm_id>/`. The actual jailer
chroot is at `<run_root>/<vm_id>/<exec basename>/<vm_id>/root/` (jailer's
hardcoded layout — see `m80-jailer`).
The public pure helpers `run_dir_path`, `firecracker_api_socket_path`,
`vsock_socket_path`, `rootfs_overlay_path`, `scratch_image_path`,
`console_log_path`, and `boot_identity_path` expose this layout for callers
and regression tests.
The Firecracker API socket and vsock muxer socket are inside the jailer
chroot, not directly in `<run_root>/<vm_id>/`.
Firecracker/jailer stdout and stderr are appended to
`<run_root>/<vm_id>/console.log`; with `console=ttyS0` this is also the
guest serial console, including m80-guestd's structured stderr lines.
Host lifecycle diagnostics are appended to `<run_root>/<vm_id>/diagnostics.jsonl`
with schema version 2. The diagnostics writer is optional: if opening or
writing it fails, boot and teardown continue and the failure is logged through
`tracing`. See `docs/behaviors/observability/diagnostics-log.md`.
`Backend::recover_stale_run_root()` is a one-shot orchestrator-driven
scan; v0.1 has no background recovery thread. Cross-process collision
avoidance: distinct `<run_root>` paths.

### Boot cmdline

`boot_args_for(image_kind, kernel_kind)` selects the kernel command line based
on the two-axis `(ImageKind, KernelKind)` matrix from the image manifest:

| `ImageKind` | `KernelKind` | Cmdline |
|---|---|---|
| `Ubuntu` | `Stock` | `console=ttyS0 reboot=k panic=-1 pci=off` |
| `Ubuntu` | `Stripped` | `console=ttyS0 reboot=k panic=-1 pci=off quiet loglevel=0 8250.nr_uarts=1` |
| `Minimal` | `Stock` | `console=ttyS0 reboot=k panic=-1 pci=off init=/m80-guestd` |
| `Minimal` | `Stripped` | `console=ttyS0 reboot=k panic=-1 pci=off quiet loglevel=0 8250.nr_uarts=1 init=/m80-guestd` |

Stripped-kernel differences from the Stock baseline:
- `quiet loglevel=0` added — suppresses per-device init messages while keeping
  the console open; fatal panics still print (bypasses loglevel).
- `8250.nr_uarts=1` added — single-UART cap; prevents probe of the four default
  UARTs. Locked at `=1` (not `=0`): preserving console output for boot-stage
  diagnostics is non-negotiable per CLAUDE.md "diagnostics before hypotheses".

`SandboxConfig::boot_args` overrides the whole cmdline when set.

### Drive layout

Firecracker assigns block-device names in PUT order: first PUT becomes
`/dev/vda`, second `/dev/vdb`, etc. m80 PUTs in this order:

| Position | drive_id          | Host file                                | RO/RW   | Guest path | Purpose |
|---------:|-------------------|------------------------------------------|---------|------------|---------|
|        1 | `rootfs`          | `<image>/output.ext4` (shared base)      | **RO**  | `/dev/vda` | Read-only base ext4. Bind-mounted into the jail at `/rootfs.ext4`. Same host file for every VM that uses this image — host page cache deduplicates. |
|        2 | `rootfs_overlay`  | `<run_dir>/rootfs.overlay.ext4`          | RW      | `/dev/vdb` | Per-VM sparse ext4. m80-guestd's PID-1 setup mounts `/dev/vda` as the lowerdir, this as the upperdir, overlayfs on `/`. |
|        3 | `workspace`       | `<run_dir>/scratch.ext4` (if requested)  | RW      | `/dev/vdc` | Per-VM workspace ext4. Mounted at `/workspace`. Subject to opt-in `Scratch::extract` after stop. Only present when `SandboxConfig::workspace_dir.is_some()`. |

The base + overlay split is what `m80-storage::Rootfs` produces;
`m80-storage::Scratch` is the workspace. Per-VM sparse files cost ~10 ms
each at most to allocate + format; there is no full-rootfs copy.

Boot artifact identity is verified before backend construction by
`m80-preflight` (`Rootfs + manifest`). `Sandbox::launch` does not recompute
kernel/rootfs/guestd sha256s in phase 3; phase 3 only prepares per-VM storage
from already-admitted artifacts.

`SandboxConfig::overlay_size_bytes` controls the sparse overlay size
(default: 512 MiB). The overlay grows as the guest writes; the sparse
allocation costs zero disk bytes at creation.

`SandboxConfig::idle_timeout` controls the idle-shutdown timer (default:
`Some(300s)`). See "Idle timeout" below.
`SandboxConfig::request_id` controls diagnostics and wire-frame correlation
for callers that already minted an opaque request id.

### Preboot REST wiring

`m80-firecracker` builds a pure ordered preboot PUT plan and applies it before
`InstanceStart`: machine config, boot source, shared read-only rootfs drive,
per-VM rootfs overlay drive, optional workspace scratch drive, then vsock.
Outbound NAT is rejected in v0.1 before this plan is built, so there is no NIC
PUT in v0.1. After the plan succeeds and before `InstanceStart`, the launch
path writes `<run_dir>/boot-identity.json` from the identity admitted by
`m80-preflight`. See `docs/behaviors/lifecycle/preboot-wiring.md`.

### Start and readiness

Cold launch starts Firecracker with `InstanceAction::InstanceStart` and waits
for guestd readiness through an inverted host listener at
`<vsock.sock>_<READY_PORT_DEFAULT>`, not by tailing the serial console. Guestd
connects to that listener and writes the m80 protocol-version byte; the host
then opens the normal exec channel on guest port 9001 before returning
`RunningSandbox`. Timeout maps to `FcError::GuestdReadyTimeout`. See
`docs/behaviors/lifecycle/start-and-ready.md`.

### Stop

`RunningSandbox::stop` is architecture-independent in v0.1: it sends
`ShutdownRequest` to guestd over vsock, then SIGKILLs the Firecracker process
after the RPC returns or fails. `RunningSandbox::force_kill` skips the guest RPC
and SIGKILLs both Firecracker and jailer pids. Both methods consume the running
handle, so repeated stop is prevented by the type-state API rather than handled
as a runtime retry. See `docs/behaviors/lifecycle/graceful-stop.md`.

### Delete and recovery

`StoppedSandbox::delete` removes the entire per-VM run directory and treats an
already-missing run-dir as clean. Recovery is the explicit
`Backend::recover_stale_run_root()` pass: live `ownership.lock` directories are
skipped, `.preserved/` triage archives are skipped, clear orphan directories
are reaped, orphaned live jail pids are killed before removal, and ambiguous
jailer or ownership-lock state is preserved. See
`docs/behaviors/lifecycle/delete-and-recovery.md`,
`docs/behaviors/cleanup/idempotent-teardown.md`, and
`docs/behaviors/concurrency/stale-detection.md`.

### Concurrency / admission

`Backend::admit().launch()` acquires one slot from the admission
semaphore (sized by `M80_MAX_CONCURRENT_VMS`, default 8). The permit is
held for the lifetime of the sandbox and dropped on `delete()` or
`preserve_for_triage()`. Admission is single-host scope; m80 does not do
multi-host placement. See `docs/behaviors/concurrency/admission.md`.

### Configuration

Loading order: built-in defaults → `/etc/m80/config.toml` →
`/etc/m80/config.d/*.toml` in lexicographic order →
`~/.config/m80/config.toml` → `~/.config/m80/config.d/*.toml` in
lexicographic order → `M80_*` env → CLI flags. Reveal the merged result via
`load_config`; callers that construct a backend from that snapshot can preserve
the source labels with `Backend::new_with_effective_config()`.
`Backend::show_effective_config()` returns the snapshot held by the backend.

Recognized config keys are `default_profile`, `max_concurrent_vms`, `run_root`,
`jail_uid`, `jail_gid`, and `cgroup_mode`. `default_profile` selects the local
runtime profile used by the CLI before preflight; the orchestrator treats it as
diagnostic/config metadata rather than a lifecycle knob. Unknown config-file
keys, unknown config.d drop-in keys, and unknown flag-override keys fail closed
with `FcError::Config`; they are not ignored.
The exact env schema is captured in
`docs/behaviors/configuration/env-schema.md`.

`load_config_from_paths(flags, ConfigFilePaths { system, system_drop_in_dir,
user, user_drop_in_dir })` uses the same precedence with explicit file paths.
It exists for hermetic callers and tests that must not read the host's real
`/etc/m80/config.toml`, `/etc/m80/config.d`, or user config.

### Idle timeout

`SandboxConfig::idle_timeout: Option<Duration>` (default: `Some(300s)`) arms
an automatic shutdown when the VM sits idle for longer than the configured
duration. A background thread spawned on `launch` (and `launch_from_snapshot`)
tracks the last `exec` activity. On expiry:

1. The thread calls `send_shutdown_request` (best-effort; logs on failure).
2. Sets an `AtomicBool` flag so the next `exec` returns `FcError::IdleTimedOut`.

`exec` updates the activity timestamp at the **start and end** of every call,
so long-running execs do not trip the watcher mid-flight. `stop()` and
`force_kill()` signal the watcher thread to exit and join it before returning.

`idle_timeout: None` disables the watcher entirely.

### Error model

Typed `FcError` variants tell the caller which phase failed; inner
causes carry detail. No silent degradation — anything that compromises
an invariant fails closed.

- `FcError::ApiSocketTimeout { path, timeout }` — Firecracker did not create
  its REST API socket during launch.
- `FcError::GuestdReadyTimeout { path, timeout }` — m80-guestd did not connect
  on the inverted-readiness socket during launch/restore.
- `FcError::IdleTimedOut` — returned by `exec` when the idle-timeout watcher
  has fired. The caller must drop or `stop()` the sandbox.
- `LifecycleFailureKind::ALL` is the bounded lifecycle vocabulary used by
  behavior docs and tests. `FcError` remains the concrete public error surface.

## Non-goals

- **No agent semantics.** No tool catalog, no `EffectClass`, no authority
  leases. m80's job is "boot a VM and run a command".
- **No multi-host placement.** Single host only.
- **No hidden cold-boot fallback in warm allocation.** `WarmPool::try_lease`
  returns `FcError::PoolEmpty` when no slot is ready.
- **No "execute and forget".** All sandboxes return through `stop()` or
  `force_kill()`.
