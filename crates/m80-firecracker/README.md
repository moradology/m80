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
Stopped while preserving the run-dir for offline inspection. If the host cannot
prove the forced kill completed, `force_kill` returns the kill error, records
`CleanupReleaseBlocker::ForcedKillAmbiguous`, and does not release the
admission permit.

The Created → Running transition runs the strict 12-phase preboot pipeline
internally (via `Sandbox::launch`). Phases are sub-steps, not states —
failures at any phase return a typed `FcError` and drop the admission permit.
When `SandboxConfig::request_id` is set, launch, request, stop, and delete
events carry that opaque id in `<run_dir>/diagnostics.jsonl`.

`Sandbox::launch_from_snapshot` is an alternative Created → Running
transition for the warm-pool restore path. It skips the full cold-boot
pipeline and instead primes the host page cache for the snapshot pair, binds
the snapshot directory into the jail, and loads that pair into a new
Firecracker process. The VM is left Running after a successful restore; the
exec channel readiness is confirmed by sending an internal lightweight exec
request over the restored vsock UDS and waiting for guestd's terminal response
(retry loop, 50 ms sleep, 5 s cap). A bare `CONNECT 9001` is not sufficient on
restore because the guest kernel can accept the socket while guestd itself is
stopped.

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

`SandboxConfig::one_shot = true` changes that reuse contract. The first user
exec or PTY request marks the running VM consumed before the guest request is
sent, and later exec attempts return `FcError::OneShotConsumed`. Warm-pool ready
probes are internal health checks and do not consume the one-shot token. When a
`WarmLease` holds a one-shot sandbox, its exec methods run the workload, then
force-kill/delete the VM and trigger pool refill before returning success. This
lets conveyor-belt callers hand off one workload without external cleanup
state. `WarmLease::attach_drive_verified` can run before that first workload so
the slot receives and verifies its tenant drive while the one-shot token remains
unconsumed.

`RunningSandbox::exec_streaming(&mut self, ..., on_chunk)` is the real-time
stdout/stderr form. It forces `ExecRequest::streaming = true`, invokes
`on_chunk` for each `ExecChunk::Stdout` / `ExecChunk::Stderr` frame, and
returns the terminal `ExecExit`. The callback is fallible; returning `Err`
closes the streaming connection, which guestd treats as cancellation for the
in-flight child. The buffered `exec` method is built on top of this path and
preserves the existing 1 MiB per-stream cap; direct streaming callers receive
all chunks until guest EOF or cancellation and observe `ExecExit::truncated =
false` unless a future explicit streaming cap is added. If the streaming caller
unwinds or otherwise drops the channel mid-request, guestd treats the disconnect
as cancellation and reaps the child before accepting the next exec.

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
normal terminal exec result. The helper thread that forwards a host cancel is
joined only after it reports completion; if it is still blocked after 100 ms,
the Drop path logs and detaches it so the exec caller cannot hang indefinitely.

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

`RunningSandbox::attach_drive_verified(self, HotplugDriveAttach)` retargets one
preallocated Firecracker drive slot with `PATCH /drives/{id}`, asks guestd to
mount that block device, and verifies the caller's opaque tenant identity bytes
before returning the still-running sandbox. `HotplugDriveAttach::path_on_host`
is the Firecracker-visible path for the new backing file; because m80 runs
Firecracker in a jail, callers must pass an absolute path that is visible inside
that jail namespace. Attach, mount, protocol, or identity failures consume the
running sandbox and force-kill/delete the VM so a partially attached tenant
drive is never returned to the caller as reusable state. Guest mount failures
map to `FcError::DriveHotplug`; byte mismatches map to
`FcError::TenantIdentityMismatch`.

`RunningSandbox::detach_drive(self, HotplugDriveDetach)` is the matching
two-phase cleanup path. It sends `DriveDetachRequest` to guestd and waits for a
bounded `DriveDetachResponse` before retargeting the Firecracker slot back to
its placeholder backing file. `Detached` and `NotMounted` are successful
idempotent guest outcomes; `Failed` maps to `FcError::DriveHotplug`. Detach,
protocol, or host cleanup failures consume and discard the VM.

After a vsock channel is established, malformed protobuf, oversized frames,
unsupported protocol versions, unexpected frame kinds, response `request_id`
mismatches, stream sequence gaps, disconnects before a required terminal
frame, and no-progress read timeouts while awaiting that terminal frame map to
`FcError::Protocol(WireProtocolError::...)`. Transport setup
failures remain `FcError::Vsock`.

`RunningSandbox::guest_metrics()` sends a direct `MetricsRequest` to
m80-guestd and returns the fixed-shape guest CPU, memory, and daemon counter
snapshot without spawning a guest process.

`RunningSandbox::ping_guest()` sends a direct `PingRequest` to m80-guestd and
returns `PongResponse { guest_unix_ms }` without spawning a guest process.

### Public lifecycle methods

| Method | Signature | Description |
|---|---|---|
| `RunningSandbox::exec` | `(&mut self, ExecRequest) -> Result<ExecResponse, FcError>` | Run one command and return buffered stdout/stderr/exit. |
| `RunningSandbox::exec_with_max_duration` | `(&mut self, ExecRequest, u64) -> Result<ExecResponse, FcError>` | Run one buffered command with an envelope call deadline. |
| `RunningSandbox::exec_with_cancel` | `(&mut self, ExecRequest, std::sync::mpsc::Receiver<()>) -> Result<ExecResponse, FcError>` | Run one buffered command and send guest cancellation when the receiver fires. |
| `RunningSandbox::exec_streaming` | `(&mut self, ExecRequest, impl FnMut(ExecChunk) -> Result<(), FcError>) -> Result<ExecExit, FcError>` | Run one command and deliver stdout/stderr chunks before terminal exit. |
| `RunningSandbox::exec_streaming_with_max_duration` | `(&mut self, ExecRequest, u64, impl FnMut(ExecChunk) -> Result<(), FcError>) -> Result<ExecExit, FcError>` | Run one streaming command with an envelope call deadline. |
| `RunningSandbox::exec_streaming_with_cancel` | `(&mut self, ExecRequest, std::sync::mpsc::Receiver<()>, impl FnMut(ExecChunk) -> Result<(), FcError>) -> Result<ExecExit, FcError>` | Streaming exec plus same-connection guest cancellation. |
| `RunningSandbox::exec_pty` | `(&mut self, PtyRequest, std::sync::mpsc::Receiver<PtyHostEvent>, impl FnMut(PtyOutputChunk) -> Result<(), FcError>) -> Result<PtyExit, FcError>` | Run one terminal command and deliver merged PTY output before terminal exit. |
| `RunningSandbox::read_file` | `(&mut self, path, max_bytes) -> Result<(Vec<u8>, bool), FcError>` | Read bytes directly from the guest and report truncation. |
| `RunningSandbox::write_file` | `(&mut self, path, bytes, mode) -> Result<u64, FcError>` | Write one guest file directly. |
| `RunningSandbox::list_dir` | `(&mut self, path) -> Result<Vec<DirEntry>, FcError>` | List one guest directory level. |
| `RunningSandbox::stat_file` | `(&mut self, path) -> Result<FileStat, FcError>` | Stat one guest path without following final symlink. |
| `RunningSandbox::remove_file` | `(&mut self, path) -> Result<(), FcError>` | Remove one non-directory guest path. |
| `RunningSandbox::create_dir` | `(&mut self, path, mode, recursive) -> Result<bool, FcError>` | Create one guest directory and report whether it was newly created. |
| `RunningSandbox::upload_file_chunked` | `(&mut self, path, mode, reader, chunk_size) -> Result<u64, FcError>` | Upload via begin/chunk/commit on one vsock connection. |
| `RunningSandbox::attach_drive_verified` | `(self, HotplugDriveAttach) -> Result<RunningSandbox, FcError>` | Retarget one preallocated drive slot, wait for guest mount ACK, verify opaque identity bytes, and discard the VM on failure. |
| `RunningSandbox::detach_drive` | `(self, HotplugDriveDetach) -> Result<RunningSandbox, FcError>` | Ask guestd to unmount a preallocated slot, then retarget the slot to its placeholder backing file. |
| `RunningSandbox::guest_metrics` | `(&mut self) -> Result<MetricsResponse, FcError>` | Read guest CPU, memory, and guestd counter metrics over vsock. |
| `RunningSandbox::ping_guest` | `(&mut self) -> Result<PongResponse, FcError>` | Round-trip a guest health probe and return the guest handling timestamp. |
| `Sandbox::launch_from_snapshot` | `(self, snapshot: SnapshotPaths, discovery: &Discovery) -> Result<RunningSandbox, FcError>` | Restore a snapshot into a new Running sandbox. |
| `RunningSandbox::capture` | `(&mut self, paths: SnapshotPaths) -> Result<(), FcError>` | Capture the live VM; leaves VM Paused and records snapshot-capture stop evidence. |
| `StoppedSandbox::run_dir` | `(&self) -> &Path` | Return the stopped VM run directory. |
| `StoppedSandbox::extract_changes` | `(&self, into: &Path) -> Result<ChangeSet, FcError>` | Extract caller-requested workspace changes from the scratch image, capped at the scratch image byte length. |
| `StoppedSandbox::delete` | `(self) -> Result<(), FcError>` | Remove the run directory and release the admission permit. |
| `StoppedSandbox::preserve_for_triage` | `(self) -> Result<PathBuf, FcError>` | Move the run directory under `.preserved/` and release the admission permit. |

`SnapshotPaths` is re-exported from `m80-snapshot` for caller convenience
because snapshot capture/restore methods are first-class `m80-firecracker`
lifecycle methods. `ChangeSet` is likewise re-exported from `m80-storage`
because `StoppedSandbox::extract_changes` returns it directly.
`SandboxConfig::request_id` is optional and opaque; it is for diagnostics and
wire-frame pairing only, not an agent semantic identifier.
`WireProtocolError` is re-exported for callers that need to distinguish broken
peer bytes from transport failures.

`SandboxConfig::network` supports three caller intents. `NoEgress` launches with
no guest NIC and no host iptables changes. `AllowOutbound` resolves to
OutboundNat: launch realizes the run-root bridge and per-VM TAP, prepares the
PID-1 `m80.net.*` boot tokens, installs the host NAT/filter policy, emits a
Firecracker `NetworkInterface` PUT for `eth0`, and records ownership state for
failure/delete cleanup. `JoinNetns { spec: NetnsSpec }` delegates namespace,
TAP, routing, and firewall ownership to the caller: m80 validates the namespace
path, passes it to Firecracker's official jailer as `--netns`, emits the
Firecracker `NetworkInterface` PUT for the caller-created TAP, and passes
static PID-1 guest network tokens for `eth0`.
`NoEgress` and `AllowOutbound` are guest networking policies, not private
network namespaces for the Firecracker VMM process; the only current VMM netns
placement promise is `JoinNetns`.

`SandboxConfig::daemonize` asks the official Firecracker jailer to double-fork
before exec'ing Firecracker. The Firecracker API socket remains the management
surface; m80 records the daemon Firecracker PID and uses `jailer_pid = 0` as the
no-live-jailer-parent sentinel.

### First-line machine shape

`FIRST_LINE_VCPU_COUNT` and `FIRST_LINE_MEM_SIZE_MIB` name the default
Firecracker shape: 1 vCPU and 512 MiB. Omitted `SandboxConfig::vcpu_count`
and `SandboxConfig::mem_size_mib` resolve to those values during preboot.
Preboot machine config also sets `smt = false` and omits `cpu_template` by
default. This is the latency-first same-host shape. Callers that need an AWS
template-masked CPU surface for snapshot portability can set
`SandboxConfig::cpu_template` to `CpuTemplate::T2` or `CpuTemplate::C3`.
Callers may still supply explicit sizing for ordinary launches.
`SandboxConfig::cpuset_cpus` optionally writes the VM cgroup leaf
`cpuset.cpus` during unified-v2 launch; `None` inherits the parent effective
CPU set. Snapshot restore and warm-pool timing fixtures use the exported
constants so latency proofs do not drift to a benchmark-only smaller VM.

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
| `WarmPoolConfig` | Target ready-slot count, snapshot pair, stateless `SandboxConfig`, ready-probe `ExecRequest`, VM id prefix, and optional `WarmPoolCpuAllocator`. |
| `WarmPoolCpuAllocator` | Optional slot-aware cgroup pinning policy. It assigns each warm slot a disjoint contiguous `cpuset.cpus` range derived from `first_cpu`, `cpus_per_slot`, and `target_ready`. |
| `WarmPool::new` | Constructs the pool; rejects `target_ready == 0` and workspace-backed configs. |
| `WarmPool::fill_to_target_blocking` | Synchronously pre-restores ready slots before serving traffic. |
| `WarmPool::start_background_fill` | Starts bounded background restore workers until ready plus filling slots cover the target. |
| `WarmPool::try_lease` | Returns a ready `WarmLease` or `FcError::PoolEmpty`; never cold-boots or restores on the allocation path. |
| `WarmPool::wait_for_ready` | Waits until a minimum ready-slot count is available or returns `PoolEmpty` / the most recent fill error on timeout. |
| `WarmPool::snapshot` | Returns `WarmPoolSnapshot` for status reporting and tests. |
| `WarmPoolSnapshot` | Observable pool counts: target, ready, filling, leased, discarded. |
| `WarmLease::exec` | Delegates one exec to the leased `RunningSandbox`. |
| `WarmLease::exec_with_request_id` | Delegates one exec while temporarily stamping the leased slot with the caller request id. |
| `WarmLease::exec_streaming` | Delegates one streaming exec to the leased `RunningSandbox`, preserving stdout/stderr chunks before terminal exit. |
| `WarmLease::exec_streaming_with_request_id` | Streaming exec plus temporary caller request-id stamping. |
| `WarmLease::attach_drive_verified` | Attaches and identity-verifies a tenant drive before the lease workload; failures release the lease and refill a replacement. |
| `WarmLease::vm_id` | Returns the leased slot VM id for diagnostics. |
| `WarmLease::run_dir` | Returns the leased slot run directory for diagnostics before discard. |
| `WarmLease::discard` | Kills and deletes the slot, then starts background refill. `Drop` performs the same discard best-effort. One-shot leases perform this after the first exec. |

The first implementation never infers reuse from liveness, process
handles, socket existence, metrics, or clean-looking directories.
Leases are discarded and replaced. A restored slot does not enter
`Ready` until its configured
ready-probe exec completes successfully. See `docs/design/warm-pool.md`
for the state machine and sizing model.

### Threat model when embedded

`m80-firecracker` can be embedded as a Rust library, but in-process embedding is
not a security boundary. Code in the same process can inspect heap state, retain
handles, call public methods, and use documented run-root paths. Callers that
need isolation from product-specific adapter code should put m80 behind a
process boundary: CLI invocation, a narrow helper process, or a service that
owns preflight discovery, admission, lifecycle, and warm-pool state.

`BackendConfig` is constructed through `BackendConfig::builder(discovery)`;
its fields are private so external callers cannot fabricate a backend config by
literal assignment. Caller-supplied `vm_id` values are admitted only when they
are 1..=64 ASCII alphanumeric, `.`, `_`, or `-` characters, are not reserved
run-root names, and fit the AF_UNIX socket path budget.

### Run-root layout

All per-VM state lives under `<run_root>/<vm_id>/`. The actual jailer
chroot is at `<run_root>/<vm_id>/<exec basename>/<vm_id>/root/` (jailer's
hardcoded layout — see `m80-jailer`).
The per-VM run directory is created owner-only (`0700`). `console.log`,
`diagnostics.jsonl`, and `boot-identity.json` are created owner-only (`0600`)
so host-local users outside the m80 owner cannot read guest console output,
artifact paths, or lifecycle diagnostics by default.
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
`Backend::new()` runs one synchronous best-effort stale run-root recovery pass.
`Backend::recover_stale_run_root()` remains available as an explicit one-shot
orchestrator-driven scan; v0.1 has no background recovery thread.
Recovery treats only valid VM-id-shaped child names as VM run dirs. Malformed
child names are preserved for manual inspection and never passed to cgroup or
network cleanup as VM identifiers.
Cross-process collision avoidance: distinct `<run_root>` paths.

### Boot cmdline behavior

The internal preboot planner selects the kernel command line based on the
two-axis `(ImageKind, KernelKind)` matrix from the image manifest and appends
`m80.workspace=0|1` so PID-1 guestd knows whether `/dev/vdc` is an actual
workspace drive:

| `ImageKind` | `KernelKind` | Cmdline |
|---|---|---|
| `Ubuntu` | `Stock` | `console=ttyS0 reboot=k panic=-1 pci=off init=/m80-guestd m80.workspace=0|1` |
| `Ubuntu` | `Stripped` | `console=ttyS0 reboot=k panic=-1 pci=off quiet loglevel=0 8250.nr_uarts=1 earlycon=uart8250,io,0x3f8,115200n8 printk.time=1 init=/m80-guestd m80.workspace=0|1` |
| `Minimal` | `Stock` | `console=ttyS0 reboot=k panic=-1 pci=off init=/m80-guestd m80.workspace=0|1` |
| `Minimal` | `Stripped` | `console=ttyS0 reboot=k panic=-1 pci=off quiet loglevel=0 8250.nr_uarts=1 earlycon=uart8250,io,0x3f8,115200n8 printk.time=1 init=/m80-guestd m80.workspace=0|1` |

Stripped-kernel differences from the Stock baseline:
- `quiet loglevel=0` added — suppresses per-device init messages while keeping
  the console open; fatal panics still print (bypasses loglevel).
- `8250.nr_uarts=1` added — single-UART cap; prevents probe of the four default
  UARTs. Locked at `=1` (not `=0`): preserving console output for boot-stage
  diagnostics is non-negotiable per CLAUDE.md "diagnostics before hypotheses".

`SandboxConfig::boot_args` is append-only. m80 always emits the selected base
cmdline, `init=/m80-guestd`, `m80.workspace=0|1`, and `m80.rootfs=<format>`
first; caller extras are appended only after those m80-owned tokens. Caller
extras that try to set m80-owned boot tokens such as `init=`,
`m80.workspace=`, `m80.rootfs=`, or `rootfstype=` fail admission with
`ConfigError::InvalidValue`.

### Drive layout

Firecracker assigns block-device names in PUT order: first PUT becomes
`/dev/vda`, second `/dev/vdb`, etc. m80 PUTs in this order:

| Position | drive_id          | Host file                                | RO/RW   | I/O engine | Cache type | Guest path | Purpose |
|---------:|-------------------|------------------------------------------|---------|------------|------------|------------|---------|
|        1 | `rootfs`          | `<image>/output.ext4` or `output.erofs` (shared base) | **RO**  | default    | default    | `/dev/vda` | Read-only base rootfs. Bind-mounted into the jail as the admitted artifact. Same host file for every VM that uses this image — host page cache deduplicates. |
|        2 | `rootfs_overlay`  | `<run_dir>/rootfs.overlay.ext4`          | RW      | `Async`    | `Unsafe`   | `/dev/vdb` | Per-VM sparse ext4. m80-guestd's PID-1 setup mounts `/dev/vda` as the lowerdir, this as the upperdir, overlayfs on `/`. |
|        3 | `workspace`       | `<run_dir>/scratch.ext4` (if requested)  | RW      | `Async`    | `Unsafe`   | `/dev/vdc` | Per-VM workspace ext4. Mounted at `/workspace`. Subject to opt-in `Scratch::extract` after stop. Only present when `SandboxConfig::workspace.is_some()`. |
|     3+N | `hotplug_slot_N`  | `<run_dir>/hotplug-slot-N.raw`           | RW      | default    | default    | next block device | Optional placeholder drive slots. Created when `SandboxConfig::preallocated_drive_slots > 0`; later callers retarget an existing slot with Firecracker `PATCH /drives/{id}`. |

The base + overlay split is what `m80-storage::Rootfs` produces;
`m80-storage::Scratch` is the workspace. Per-VM sparse files cost ~10 ms
each at most to allocate + format; there is no full-rootfs copy.

Preallocated hotplug slots are a latency/security trade-off. Each slot is a
live writable virtio-blk device attached before `InstanceStart`, even while it
points at an empty placeholder image. The default is zero; callers that set a
larger value get faster later tenant-drive attach at the cost of extra
Firecracker block-device surface for the VM lifetime.

Boot artifact identity is verified before backend construction by
`m80-preflight` (`Rootfs + manifest`). `Sandbox::launch` does not recompute
kernel/rootfs/guestd sha256s in phase 3; phase 3 only prepares per-VM storage
from already-admitted artifacts.

`SandboxConfig::overlay_size_bytes` controls the sparse overlay size
(default: 512 MiB). The overlay grows as the guest writes; the sparse
allocation costs zero disk bytes at creation.

`SandboxConfig::drive_cache_type` overrides the writable preboot drive cache
policy. `None` uses the m80 default, `CacheType::Unsafe`, for the ephemeral
overlay and workspace drives. Set `Some(CacheType::Writeback)` when a caller
needs Firecracker's conservative host sync behavior.

`SandboxConfig::idle_timeout` controls the idle-shutdown timer (default:
`Some(300s)`). See "Idle timeout" below.
`SandboxConfig::request_id` controls diagnostics and wire-frame correlation
for callers that already minted an opaque request id.
`SandboxConfig::one_shot` controls destroy-after-use behavior for warm conveyor
slots. It defaults to `false`.

### Preboot REST wiring

`m80-firecracker` builds a pure ordered preboot PUT plan and applies it before
`InstanceStart`: machine config with an optional caller-selected CPU template,
boot source, shared read-only rootfs drive, per-VM rootfs overlay drive,
optional workspace scratch drive, optional preallocated hotplug drive slots,
optional network interface for an OutboundNat TAP, virtio-rng entropy device,
then vsock. Before the first REST PUT, launch opens the Firecracker API socket
by watching the run directory for socket creation rather than polling on a
fixed interval. Boot-source args append `m80.workspace=<0|1>` and
`m80.rootfs=<ext4|erofs>` from the admitted manifest; erofs also adds
`rootfstype=erofs` for the kernel's initial root mount. Outbound NAT
boot-source args append the prepared `m80.net.*` PID-1 tokens after those m80
markers. After the plan succeeds and before `InstanceStart`, the launch path writes
`<run_dir>/boot-identity.json` from the identity admitted by `m80-preflight`.
See `docs/behaviors/lifecycle/preboot-wiring.md` and
`docs/behaviors/lifecycle/virtio-rng.md`.

Preallocated slots are opt-in and default to zero because
ordinary one-shot launches do not need extra block devices. The slot exists
only so request-path attach is a `PATCH /drives/{id}` against a previously
PUT drive, not a create-then-attach operation. Firecracker versions that do
not support drive `PATCH` fail through the `m80-firecracker-client`
`DriveWriteFailed` path.

### Start and readiness

Cold launch starts Firecracker with `InstanceAction::InstanceStart` and waits
for guestd readiness through an inverted host listener at
`<vsock.sock>_<READY_PORT_DEFAULT>`, not by tailing the serial console. Guestd
connects to that listener and writes the m80 protocol-version byte; the host
accepts that connection through `poll(2)` readiness instead of a host-side
sleep loop. The launch path then returns `RunningSandbox`; the caller's first
operation opens the normal exec channel on guest port 9001. Timeout maps to
`FcError::GuestdReadyTimeout`. See
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
already-missing run-dir as clean. Recovery runs once during `Backend::new()` and
remains available through the explicit `Backend::recover_stale_run_root()` pass:
live `ownership.lock` directories are skipped, `.preserved/` triage archives are
skipped, the `warm/` owner control tree is skipped, clear orphan directories
are reaped, orphaned live jail pids are killed before removal, owned network
state is cleaned before `network-state.json` is deleted, and ambiguous jailer
or ownership-lock state is preserved. See
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

- `FcError::Config(ConfigError)` carries structured configuration failures via
  the `ConfigError` enum (`TomlSyntax`, `MissingField`, `InvalidValue`,
  `VmIdPathBudgetExceeded`). It is not a fallback bucket: it is reserved for
  caller configuration, CLI flag, and config merge failures where no lower crate
  owns a more specific typed cause.
- `ConfigError::VmIdPathBudgetExceeded` is raised at `Backend::admit()` when a
  caller-supplied `vm_id` would produce an AF_UNIX socket path longer than the
  kernel `sun_path` cap (107 usable bytes). The path layout is
  `<run_root>/<vm_id>/<fc_basename>/<vm_id>/root/firecracker.sock`; vm_id
  appears twice because the jail layout inherits Firecracker's jailer
  convention. The check is pure arithmetic and runs before the admission
  permit is acquired — over-budget admits never consume a permit.
- `Backend::admit()` also rejects caller-supplied `vm_id` values that collide
  with reserved run-root children (`.preserved`, `warm`) before consuming a
  permit.
- `FcError::ApiSocketTimeout { path, timeout }` — Firecracker did not create
  its REST API socket during launch.
- `FcError::GuestdReadyTimeout { path, timeout }` — m80-guestd did not connect
  on the inverted-readiness socket during launch/restore.
- `FcError::RunDirOwnershipAmbiguous`, `RunDirAlreadyOwned`, and
  `RunDirNotFound` distinguish run-root admission/walk failures.
- `FcError::PathIo`, `Json`, `CommandSpawnFailed`, `CommandFailed`, and
  `ArtifactMissing` preserve concrete host paths, serialization contexts, and
  helper-command status instead of collapsing them into config strings.
- `FcError::UnsupportedOperation` names an unavailable v0.x API surface without
  pretending the caller supplied bad configuration.
- Warm-pool/owner failures use typed variants (`WarmPoolFillFailed`,
  `WarmReadyProbeRejected`, `WarmReadyProbeNoResult`,
  `WarmOwnerSocketExists`, `WarmOwnerNotAcceptingLeases`,
  `WarmOwnerDrainTimeout`, `WarmCompatibilityMismatch`,
  `UnexpectedWarmResponse`) so CLI IPC and owner-state failures remain
  distinguishable from configuration.
- `FcError::IdleTimedOut` — returned by `exec` when the idle-timeout watcher
  has fired. The caller must drop or `stop()` the sandbox.
- `LifecycleFailureKind::ALL` is the bounded lifecycle vocabulary used by
  behavior docs and tests. `FcError` remains the concrete public error surface.

## Public surface

Core types:

- `Sandbox` — pre-launch handle; owned by `Backend::admit().launch()`.
- `RunningSandbox` — live VM handle; all exec/file-op/PTY methods live here.
- `StoppedSandbox` — post-stop handle; carries `delete()` and `preserve_for_triage()`.
- `Backend` — orchestration root: `new(BackendConfig)`,
  `new_with_effective_config(BackendConfig, EffectiveConfig)`, `config()`,
  `admit()`, `show_effective_config()`, and `recover_stale_run_root()`.
- `WarmPool` — pre-restored ready-slot pool; leases `WarmLease`.
- `WarmPoolCpuAllocator` — optional disjoint `cpuset.cpus` range allocator for
  warm-pool slots.
- `WarmPoolSnapshot` — observable warm-pool counts.
- `WarmLease` — single exec slot checked out from `WarmPool`.
- `SandboxConfig` — per-VM launch parameters (request id, cpuset pin, overlay size, idle timeout, daemonize, preallocated drive slots, one-shot mode, etc.).
- `CpuTemplate` — re-exported Firecracker CPU template enum for callers that
  opt into `SandboxConfig::cpu_template`.
- `CacheType` — re-exported Firecracker drive cache enum for callers that opt
  into `SandboxConfig::drive_cache_type`.
- `HotplugDriveAttach` — host-side request to attach and verify one preallocated drive slot.
- `HotplugDriveDetach` — host-side request to detach one preallocated drive slot.
- `BackendConfig` — host-level config (run root, jail uid/gid, admission limit, etc.).
- `EffectiveConfig` — merged snapshot returned by `load_config` and held by `Backend`.
- `FcError` — exhaustive typed error for all phases.

Re-exports for callers:

- `SnapshotPaths` (from `m80-snapshot`; intentional lifecycle ergonomics).
- `NetworkPolicy` and `NetnsSpec` (from `m80-net-mode`).
- `WireProtocolError` (defined by `m80-firecracker`).
- `ExecChunk`, `PtyHostEvent`, `PtyOutputChunk` (defined by `m80-firecracker`).
- `ChangeSet` (from `m80-storage`; intentional stopped-sandbox ergonomics).

Wire request/response types such as `ExecRequest`, `ExecResponse`,
`ExecExit`, `ExecStatus`, `ExecTiming`, `PtyRequest`, and `FileError` are owned
by `m80-proto`; callers import them directly from that crate.

Config helpers:

- `load_config(flags) -> EffectiveConfig`
- `load_config_from_paths(flags, ConfigFilePaths) -> EffectiveConfig`
- `BackendConfig::builder(discovery)` for constructing backend config from
  preflight discovery plus explicit overrides.

Layout helpers:

- `run_dir_path`, `firecracker_api_socket_path`, `vsock_socket_path`,
  `rootfs_overlay_path`, `scratch_image_path`, `console_log_path`,
  `boot_identity_path`.
- `BOOT_IDENTITY_FILE`, `CONSOLE_LOG`, `FIRECRACKER_API_SOCKET`,
  `ROOTFS_OVERLAY_IMAGE`, `SCRATCH_IMAGE`, `VSOCK_SOCKET`, and
  `OWNERSHIP_LOCK`.

Config defaults:

- `FIRST_LINE_VCPU_COUNT`, `FIRST_LINE_MEM_SIZE_MIB`.
- `SandboxConfig::cpu_template = None`.

Cleanup vocabulary (behavior docs + regression tests):

- `CleanupPhase` / `CLEANUP_PHASE_ORDER`.
- `StopDisposition` / `STOP_DISPOSITIONS`.
- `CleanupReleaseBlocker` / `CLEANUP_RELEASE_BLOCKERS`.
- `CleanupAuthority` / `CLEANUP_AUTHORITY`.
- `LifecycleFailureKind::ALL`.

Warm pool:

- `WarmPoolConfig`.
- `WarmPoolCpuAllocator`.
- `WarmPoolSnapshot`.

## Non-goals

- **No agent semantics.** No tool catalog, no `EffectClass`, no authority
  leases. m80's job is "boot a VM and run a command".
- **No multi-host placement.** Single host only.
- **No hidden cold-boot fallback in warm allocation.** `WarmPool::try_lease`
  returns `FcError::PoolEmpty` when no slot is ready.
- **No "execute and forget".** All sandboxes return through `stop()` or
  `force_kill()`.

## Dependencies

- `m80-firecracker-client` — Firecracker REST API calls.
- `m80-vsock` — vsock channel open and frame transport.
- `m80-jailer` — jail chroot setup and teardown.
- `m80-cgroup` — cgroup v2 subtree lifecycle.
- `m80-storage` — rootfs overlay and scratch image preparation.
- `m80-preflight` — host capability verification at backend construction.
- `m80-net-mode` — network policy resolution.
- `m80-net-outbound` — outbound NAT bridge/TAP, guest config, iptables, and cleanup.
- `m80-snapshot` — snapshot path types.
- `m80-proto` — wire types re-exported for callers.
- `m80-image-manifest` — image kind / kernel kind for boot-args selection.
- `serde`, `serde_json`, `thiserror`, `tracing`, `toml`.

## Debug instrumentation

`M80_DEBUG_WIRE` is the global wire-trace knob. The following targets are
recognized across the workspace:

| Target | Crate that honors it | What it traces |
|--------|----------------------|----------------|
| `vsock` | `m80-vsock` | vsock handshake lines, outbound frame previews up to 1024 bytes, and inbound frame kinds; `exec_request` env entries are redacted before preview formatting |
| `fcrest` | `m80-firecracker-client` | Firecracker REST PUT request (method, path, body) and response (status, body), with hex+ASCII preview up to 1024 bytes |
| `all` | both | enables all targets |

Usage rules (identical in both crates):

- Matching is exact (`==`): `M80_DEBUG_WIRE= vsock` (leading space) does not match.
- Multiple targets are comma-separated: `M80_DEBUG_WIRE=vsock,fcrest`.
- Unknown tokens are silently ignored.
- The gate is a single atomic load on the hot path; no serialization occurs unless it fires.
- `vsock` redacts `ExecRequest.env` values in debug previews. `fcrest` is a raw
  Firecracker REST dump and should only be enabled when those request and
  response bodies are safe to log.

## Tests

- `src/preboot.rs` tests — preboot PUT order, boot-source cmdline behavior,
  drive order, default CPU-template omission, explicit CPU-template opt-in,
  and network-interface placement.
- `tests/config_loading.rs` — precedence chain, drop-in ordering, unknown-key rejection, and env-override isolation using `load_config_from_paths` and in-memory fixtures.
- `tests/cleanup_vocabulary.rs` — `CleanupPhase`, `StopDisposition`, `CleanupReleaseBlocker`, `CleanupAuthority`, and `LifecycleFailureKind::ALL` are exhaustive and match behavior docs.
- `tests/layout.rs` — pure path helpers produce expected strings given fixed run-root + vm-id inputs.
- `tests/warm_pool.rs` — `WarmPool::new` rejects `target_ready == 0` and workspace-backed configs; `BlankVmResetEvidence` fields are exhaustively named.
- KVM integration tests (`#[ignore]`) live in `tests/end_to_end_real_kvm.rs`, `tests/warm_pool.rs`, and related lifecycle files; they require a real Firecracker binary and KVM device. The Bestiary stand-in proof is `tests/bestiary_stand_in_real_kvm.rs`; cgroup memory enforcement is pinned by `tests/cgroup_memory_oom_real_kvm.rs`.
