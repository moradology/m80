# 05 — Consumers and integration seams in predecessor

This file maps every place inside predecessor that touches `agent-sandbox-firecracker`
or builds a `SandboxBackend`. The point is to understand what re-stubbing
costs after extraction.

## Direct vs. trait consumers

### Direct consumers (import concrete `FirecrackerBackend` types)

There are **only two**:

#### 1. `services/worker-rs/src/executor_factory.rs`

Lines 31-34, 198-247.

```
FirecrackerExecutorFactory::new(
    workspace_base_path,
    timeout_ms,
    placement_state,
    backend_config: FirecrackerBackendConfig,
    policy_enforcement_mode,
    workspace_host_ownership,
) -> SandboxToolExecutor
```

Wraps `FirecrackerBackend` in `TrackingFirecrackerBackend` (a struct
implementing `SandboxBackend`) that calls
`placement_state.observe_materialized(workspace_id)` after every
execute. This wires firecracker into worker-rs's placement-tracking
system.

Per-request pattern: each tool execution gets a fresh executor via
`factory.build(&context)` (lines 389-419).

Config flow: `validate_firecracker_executor_config()` (lines 651-655)
reads from `WorkerConfig`, calls `firecracker_backend_config()`
(lines 657-670), which calls
`FirecrackerBackendConfig::from_discovery_with_modes()` with a
`DiscoveryConfig` (lines 672-690).

**To re-stub for extraction**:
- Move `TrackingFirecrackerBackend` into `m80-adapter`
- Change config wiring to use m80's config types
- Keep the placement-state callback in the tracking wrapper

#### 2. `services/sandbox-executor-rs/src/lib.rs`

Lines 46-51, 243, 306-347, 1439-1492.

```
build_backend_for_config(config) -> WorkspaceOwnershipSandboxBackend
  -> ConfiguredSandboxBackend::Firecracker(
       AdmissionLimitedSandboxBackend::new(
         FirecrackerBackend::new(runtime_config),
         max_concurrent_vms
       )
     )
```

Singleton pattern: built once at service startup, reused for all
requests. `AdmissionLimitedSandboxBackend` (lines 1330-1354) gates
concurrency via semaphore.

Config from env:
- `SANDBOX_EXECUTOR_FIRECRACKER_BIN`
- `SANDBOX_EXECUTOR_FIRECRACKER_JAILER_BIN`
- `SANDBOX_EXECUTOR_FIRECRACKER_KERNEL_IMAGE`
- `SANDBOX_EXECUTOR_FIRECRACKER_ROOTFS_IMAGE`
- `SANDBOX_EXECUTOR_FIRECRACKER_RUN_ROOT`
- `SANDBOX_EXECUTOR_FIRECRACKER_MAX_CONCURRENT_VMS`

Also runs a background task:
`start_firecracker_run_root_recovery_loop()` at lines 498, 558-580.
Calls `agent_sandbox_firecracker::recover_stale_run_root()` every 5
seconds to reap orphans.

**To re-stub for extraction**:
- Replace `FirecrackerBackend::new(...)` with adapter call
- Run-root recovery: m80 ships `m80::recover_stale_run_root()`; the
  service spawns it the same way

### Trait-only consumers

Everyone else only depends on `agent-sandbox-api::SandboxBackend`. No
firecracker import, no firecracker knowledge.

| Crate | Where |
|---|---|
| `agent-sandbox-executor-core` | `lib.rs:6` |
| `agent-sandbox-executor-nats` | `lib.rs:17` |
| `agent-tool-executor` | All files |
| `services/guestd-rs` | (in-VM, irrelevant to host extraction) |

These don't change at all. The `SandboxBackend` trait stays where it is;
only the implementation it dispatches to changes.

## Lifecycle hooks: who calls create/start/stop/delete?

**Nobody, externally.** The `SandboxBackend` trait collapses everything
into `execute()`. Inside `execute()`, `FirecrackerBackend` does:

1. `lifecycle::create_vm(...)`
2. `lifecycle::start(...)`
3. Send `GuestRequest` over vsock; await `GuestResponse`
4. `lifecycle::stop(...)` (graceful, with forced-kill fallback)
5. Optionally writeback if `EffectClass::Mutating`
6. `lifecycle::delete(...)` (cleanup of run dir, sockets, taps, etc.)

Each `execute()` call is one full VM lifecycle. There is no warm pool
in production today (`blank_pool.rs` exists but is unwired). There is
no long-lived VM.

**For m80**: this is a v0 pattern. m80 v0.2 should expose explicit
create/start/exec/stop/delete so the CLI can `m80 shell` against a
long-lived VM.

## Snapshot integration

`agent-workspace-snapshot` ↔ `agent-sandbox-firecracker`:

The two layers are **decoupled**:
- `agent-workspace-snapshot` handles checkpoint lineage and artifact
  storage at the workspace level
- `agent-sandbox-firecracker::snapshot` handles VM-level snapshot
  metadata (VmState, Memory, RuntimeRootfs, WorkspaceScratch)

Crucially: **firecracker's snapshot module is currently unwired**. The
worker-level restore path (`worker-rs/src/restore.rs`) restores
**workspace** snapshots, not VM snapshots. The bidirectional dependency
described in some docs doesn't exist in practice yet.

## Test footprint

128 references to `LEGACY_FC_INTEGRATION` or
`LEGACY_K8S_FIRECRACKER_*` across the codebase. Most live with the
crate itself; a few external tests reference real firecracker:

| Test file | Notes |
|---|---|
| `services/worker-rs/tests/kubernetes_firecracker_tool_proof.rs` | Full K8s end-to-end; gated by `LEGACY_K8S_FIRECRACKER_*` env vars |
| `services/worker-rs/tests/control_stack_worker_restart_replay.rs` | Skips if `LEGACY_FC_INTEGRATION` unset |
| `services/worker-rs/src/network_e2e_tests.rs:610-650` | `firecracker_guest_crash_recovery_smoke`, `firecracker_response_path_recovery_smoke`; both check `LEGACY_FC_INTEGRATION=1` |

The crate-internal tests (`tests/backend_conformance.rs`,
`tests/guest_control.rs`, `tests/minimal_boot.rs`,
`tests/no_egress.rs`, `tests/outbound_nat.rs`,
`tests/workspace_writeback.rs`) move with the crate.

The external-to-crate tests need re-stubbing against the adapter.
Probably 1-2 days of work.

## Kubernetes deployment

Real production deployment uses K8s with privileged DaemonSets.

### `infra/kubernetes/workloads/predecessor/base/sandbox-executor-daemonset.yaml`

- `runAsUser: 0`, `runAsGroup: 0` (lines 69-70)
- `privileged: true`, `allowPrivilegeEscalation: true` (lines 66-67)
- `/dev/kvm` mount (lines 79-80)
- `/opt/firecracker` read-only host mount (lines 76-78)
- Run root at `/var/lib/predecessor-run` (lines 84-87)

### `infra/kubernetes/workloads/predecessor/overlays/eks/worker-service-eks.yaml`

- Node selector: `predecessor.firecracker: "true"` (line 10)
- Taint toleration: `predecessor.firecracker=true:NoSchedule` (lines 12-15)
- Same run root mount

**For m80**: don't ship K8s manifests in v0.1. Document the privilege
requirements; let users wire their own deployment.

## Bleed points if extracted

| Area | Cost |
|---|---|
| `worker-rs` factory wiring | Mechanical: 1 day |
| `sandbox-executor-rs` factory wiring | Mechanical: 1 day |
| `worker-rs` placement-state tracking | Move to adapter: 0.5 day |
| Run-root recovery background task | Reroute to m80 API: 0.5 day |
| Test re-stubbing | 1-2 days |
| Snapshot integration | Currently zero coupling; defer |
| K8s manifests | Rename only: 0.5 day |

**Total cutover cost on the predecessor side: ~5 days.** Not the bottleneck.

## What stays unchanged

- `agent-sandbox-api` (the `SandboxBackend` trait)
- `agent-guest-proto` (the wire protocol)
- `agent-sandbox-tool-catalog` (the six tools)
- `agent-tool-executor` (the host dispatch + policy gate)
- `agent-sandbox-local`, `agent-sandbox-container` (sibling backends)
- `agent-guestd-lib`, `services/guestd-rs` (the in-VM daemon)

predecessor's overall architecture doesn't shift. It just stops owning the
firecracker backend; the adapter registers `m80` against the
`SandboxBackend` seam.
