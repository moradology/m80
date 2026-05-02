# 07 — Module-by-module: essential vs. production hygiene vs. drop

Source: `crates/sandbox/agent-sandbox-firecracker/src/`. Classification
into three buckets — **must-have** (cannot ship without), **hygiene**
(production polish, optional), and **drop** (predecessor-specific or unwired
in a generic lib).

## Module table

| Module | LOC | Bucket | Notes |
|---|---|---|---|
| `foundation.rs` | 2,475 | **Must-have** | Binary discovery, host preflight, privileged-command runner, env var resolution |
| `boot.rs` | 1,226 | **Must-have** | Pre-boot wiring: machine-config, boot source, drives, vsock, network |
| `lifecycle.rs` | 1,717 | **Must-have** | State machine: create / start / stop / delete |
| `client.rs` | 896 | **Must-have** | Firecracker UDS REST API client (BootSource, Drive, NIC, InstanceAction) |
| `vsock.rs` | 410 | **Must-have** | Vsock device config, ready-marker probe |
| `storage.rs` | 992 | **Must-have** | ext4 image cloning, scratch image hydration, admissibility scan, writeback extraction |
| `errors.rs` | 949 | **Must-have (trimmed)** | ~600 LOC of portable preflight/lifecycle errors; ~350 LOC of predecessor-shaped diagnostic errors to drop |
| `lib.rs` | 129 | **Must-have** | Re-export surface; rewrite for new public API |
| `backend.rs` | 1,281 | **Must-have (rewritten)** | The `SandboxBackend::execute` glue; m80 replaces with explicit lifecycle methods |
| `network.rs` | 3,669 | **Must-have for OutboundNat / drop for NoEgress-only** | See file 06; defer for v0.1 |
| `jailer.rs` | 1,457 | **Must-have for production** | Jailer chroot, UID/GID drop, asset binding plan |
| `cgroup.rs` | 278 | **Hygiene** | Cgroup v2 limits; behind feature flag |
| `snapshot.rs` | 997 | **Drop** | Defined but never invoked; `UnsupportedSnapshotLaunchMode` proves it |
| `blank_pool.rs` | 1,412 | **Drop** | Exported but `backend.rs` never instantiates the allocator |
| `diagnostics.rs` | 1,164 | **Hygiene** | Structured lifecycle event log; optional |
| `ops_metrics.rs` | 455 | **Hygiene** | Per-VM metrics aggregation from disk |
| `scrape.rs` | 729 | **Hygiene** | Prometheus text + JSON health renderers |
| `probe.rs` | 549 | **Hygiene** | Run-root scanner that classifies per-VM operator state |
| `health.rs` | 285 | **Hygiene** | Wraps probe records into workspace health views |
| `readiness.rs` | 317 | **Hygiene** | Fleet readiness summary |
| `*_tests.rs` (inline) | 5,427 | **Test-only** | Co-located unit tests; some tests survive, some are predecessor-shaped |

Sums (excluding tests):
- Must-have core: **15,386 LOC**
- Hygiene tail: **~3,800 LOC**
- Drop: **~2,400 LOC**

Excluding `network.rs` (defer to v0.2): must-have core drops to
**~11,700 LOC**.

## Per-module elevator pitches

### foundation.rs (2,475 LOC) — Must-have

Discovers Firecracker/jailer binaries, validates host (Linux/KVM, run
root writable, helpers present), runs privileged commands, builds
`DiscoveryConfig` and `FirecrackerBackendConfig`. The crate's "first
contact with the host" layer.

Coupling: zero predecessor concepts beyond `FirecrackerError`. Direct port.

### boot.rs (1,226 LOC) — Must-have

Pre-boot orchestration: assemble machine-config (vCPU, memory, smt),
boot source (kernel path, kernel cmdline), root drive (rootfs.ext4),
secondary scratch drive (workspace.ext4), vsock config, optional NIC for
OutboundNat. Calls into `client.rs` to PUT each piece via Firecracker's
UDS API. Records the boot identity tuple (`boot-identity.json`) on
success.

Coupling: minimal. References `WorkspaceId`/`RunId` only for the
boot-identity record's metadata fields. Direct port with renames.

### lifecycle.rs (1,717 LOC) — Must-have

The state machine. `create_vm` (runs preflight, sets up run dir),
`start` (boots, waits for ready marker), `stop` (graceful with
SendCtrlAltDel on x86_64, forced fallback on aarch64), `delete`
(cleanup). Uses `boot.rs`, `client.rs`, `vsock.rs`, `storage.rs`,
`network.rs`, `jailer.rs`. Imports `snapshot.rs` types but doesn't call
them.

Coupling: snapshot import is dead; remove. WorkspaceId/RunId are carried
through but never gate behavior. Direct port.

### client.rs (896 LOC) — Must-have

Synchronous UDS HTTP client for Firecracker's REST API. Types for:
- `BootSourceConfig`
- `DriveConfig`
- `NetworkInterfaceConfig`
- `MachineConfig`
- `VsockConfig`
- `InstanceAction` (start / send-ctrl-alt-del)

Direct port. Pure generic.

### vsock.rs (410 LOC) — Must-have

Vsock CID derivation (per-VM, derived from `vm_id`), guest port (9001),
serial ready marker (`GUESTD_READY`), the host-side bridge socket
(`vsock.sock`). Plus the ready-marker probe loop that confirms the guest
booted.

Coupling: `GUESTD_READY` is a predecessor-specific marker string. Rename to
`M80_READY` or similar. Otherwise pure protocol mechanics.

### storage.rs (992 LOC) — Must-have

ext4 image management:
- Per-VM rootfs clone from the managed base image
- Per-VM scratch ext4 image creation, hydrated from host workspace
- Admissibility scan (only regular files + dirs survive writeback;
  symlinks, devices, fifos rejected)
- Writeback extraction via `e2fsck` + `debugfs` after clean stop
- Atomic swap into host workspace with rollback on failure

Coupling: the writeback model is opinionated (see 01-coupling-audit.md).
For m80 v0.1, ship a simpler "scratch image is opaque, you can extract
or discard" interface. The mechanism is portable.

### errors.rs (949 LOC) — Must-have (trimmed)

`FirecrackerError` is the crate-internal error type. Variants split
roughly:

- **Portable** (~600 LOC): `FirecrackerBinaryNotFound`,
  `KvmUnavailable`, `UnsupportedHostPlatform`,
  `UnsupportedFirstLineVmSizing`, `InsufficientRunRootCapacity`,
  `CgroupRequiresJailer`, `PrivilegedJailerLaunchUnavailable`,
  `BootSourceWriteFailed`, `MachineConfigWriteFailed`, `VsockNotReady`,
  `IpCommandFailed`, `IptablesCommandFailed`, etc.

- **Predecessor-shaped** (~350 LOC): `SerializeHealthSnapshot`,
  `ReadMetricsSnapshot`, `SerializeMetricsSnapshot`,
  `InvalidBlankVmResetEvidence`, `ArchiveWriteFailed`,
  `ArchivePruningFailed`, `UnsupportedSnapshotLaunchMode`. Drop these.

Conversion to `SandboxError` lives at lines 136-146; m80 replaces with
its own surface error type.

### backend.rs (1,281 LOC) — Must-have (rewritten)

Implements `SandboxBackend::execute` for `FirecrackerBackend`. Glues
together all the lifecycle pieces into a single RPC-shaped call.
Includes the `FirecrackerWritebackAuthority` trait hook at lines 66-71.

For m80, **rewrite this layer**: explicit `Sandbox` trait with
`create/start/exec/stop/delete`, no collapsed-RPC. The internal logic
(the actual sequence of operations) ports nearly 1:1.

### jailer.rs (1,457 LOC) — Must-have for production

Materializes Firecracker's official jailer chroot:
- Jail root path (under `/var/tmp/predecessor-fc-jailer` or run-root sibling)
- Jailed UID/GID = `3000/3000` (fixed by predecessor; configurable in m80)
- Asset binding plan (`BindRo` for kernel, `BindRw` for run dir,
  `CreateInsideJail` for sockets)
- Plan persisted as `jailer-plan.json`; runtime state as
  `jailer-state.json`
- Both `jailer_pid` and `firecracker_pid` recorded for startup
  scavenging

Coupling: minimal. Hardcoded predecessor jail-root path needs renaming.

### cgroup.rs (278 LOC) — Hygiene

Per-VM cgroup v2 subtree under `/sys/fs/cgroup/predecessor-firecracker`,
assigns both jailed pids, persists materialized cgroup path. Behind
`FIRECRACKER_CGROUP_MODE=unified-v2` flag. Limits CPU, memory, pids.

For m80: behind a feature flag. Document that hosts need cgroup v2
unified mode.

### snapshot.rs (997 LOC) — Drop

Defines `FirecrackerSnapshotManifest`, `FirecrackerRestoreMetadata`,
`FirecrackerSnapshotArtifact` enums. Hashes artifact sets. **Never
invoked from `lifecycle.rs`** — `lifecycle.rs:35-38` imports the types
but no function body calls `read_snapshot_manifest` or
`write_snapshot_manifest`. The error variant
`UnsupportedSnapshotLaunchMode` (lines 168 in errors.rs) explicitly
flags "not implemented for current launch path."

**Verdict**: drop in v0.1. If snapshot/restore matters, build it fresh
in v0.2 against the current Firecracker `/snapshot/load` API.

### blank_pool.rs (1,412 LOC) — Drop

Implements warm-pool reset logic: `BlankVmResetProof`,
`BlankVmResetRequirement`, eight nested proof checks (DiagnosticsClean,
GuestControlProbe, ...). Exported from `lib.rs:43-46` but **`backend.rs`
has zero references** (verified by grep).

**Verdict**: drop. m80 v0.2 may add a warm-pool concept, but it would
be smaller and less predecessor-shaped.

### diagnostics.rs (1,164 LOC) — Hygiene

Records structured lifecycle events (`DiagnosticPhase`:
StartupScavenge, HostPreflight, StoragePrepare, Boot, Ready, Request,
Stop, Writeback, Delete) into `<run_dir>/diagnostics.jsonl`. Triage
bundle reader. Wrapped in `Option<VmDiagnostics>` in `lifecycle.rs:72`,
so removing it doesn't break boot.

For m80: optional, behind a feature flag. The structured-event pattern
is good prior art; ship a simpler version in v0.2.

### ops_metrics.rs (455 LOC) — Hygiene

Aggregates per-VM `metrics.json` artifacts on disk into bounded latency
families (create_start, request_roundtrip, stop, writeback, delete) and
failure counts. Read-only.

For m80: optional. v0.2.

### scrape.rs (729 LOC) — Hygiene

Renders Prometheus text format from ops-metrics + health surfaces.
Renders JSON health snapshot. Does not run an HTTP server.

For m80: optional. v0.2 if at all.

### probe.rs (549 LOC) — Hygiene

Walks `<run_root>/*/`, reads ownership markers and lease files, checks
vsock and API-socket reachability, peeks at last failure kind. Emits
`FirecrackerVmProbeRecord` with health classification.

For m80: optional. v0.2.

### health.rs (285 LOC) — Hygiene

Thin wrapper over probe.rs that maps probe records to workspace health
views (ready/stuck flags).

For m80: optional. v0.2.

### readiness.rs (317 LOC) — Hygiene

Summarizes health views into rollout-readiness counts. Used to answer
"is it safe to roll out?"

For m80: optional. v0.2 if at all (a CLI doesn't need rollout
readiness).

## Recommended split for v0.1

| Bucket | Modules | LOC |
|---|---|---|
| **Ship in v0.1** | foundation, boot, lifecycle, client, vsock, storage, errors (trimmed), lib, backend (rewritten), jailer | ~10,000 |
| **Defer to v0.2** | network, cgroup, diagnostics, snapshot (real impl), blank_pool (real impl) | ~5,800 |
| **Drop entirely** | ops_metrics, scrape, probe, health, readiness | ~2,300 |

v0.1 ships at roughly **10k LOC of source** + tests + image build
infra. Maintainable for a small team.

## Where the lines come from

Verified by `wc -l` of `crates/sandbox/agent-sandbox-firecracker/src/`:

```
1281 backend.rs
1412 blank_pool.rs
1226 boot.rs
 278 cgroup.rs
 896 client.rs
1164 diagnostics.rs
 739 diagnostics_tests.rs
 949 errors.rs
2475 foundation.rs
1403 foundation_tests.rs
 285 health.rs
1457 jailer.rs
1305 jailer_tests.rs
 129 lib.rs
1717 lifecycle.rs
2789 lifecycle_tests.rs
3669 network.rs
 455 ops_metrics.rs
 549 probe.rs
 317 readiness.rs
 729 scrape.rs
 997 snapshot.rs
 992 storage.rs
 410 vsock.rs
27623 total
```
