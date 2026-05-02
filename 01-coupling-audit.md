# 01 — Coupling audit: predecessor-specific vs. generic in `agent-sandbox-firecracker`

## Scope

Crate: `/tank/projects/predecessor/crates/sandbox/agent-sandbox-firecracker/`
Source LOC: **27,623** across 25 files.

The question this file answers: of those 27k lines, how much is predecessor-glue
that has to be unwound for an extraction, and how much is generic VM
mechanics that travels as-is?

## Cargo dependencies (direct)

From `Cargo.toml`:

```
agent-domain         (workspace)
agent-ids            (workspace)
agent-sandbox-api    (workspace)
agent-guest-proto    (workspace)
async-trait          (workspace)
chrono               (workspace)
serde, serde_json    (workspace)
tempfile             (workspace)
thiserror            (workspace)
tokio                (workspace)
tracing              (workspace)
fs2                  (workspace)
sha2                 (workspace)
rustix               (= "1", features = ["process"])
```

Four predecessor crates, ten generic deps. The generic deps are all directly
portable.

## What each predecessor dependency actually carries

### `agent-ids` — opaque identifier types

**Imported types**: `WorkspaceId`, `RunId`, `ToolCallId`, `CorrelationId`,
`IdempotencyKey`, plus a handful of sequence wrappers.

**Where used**:
- `vsock.rs:2` — request envelopes
- `network.rs:85` — network state JSON records
- `backend.rs:34-38` — `FirecrackerWritebackAuthorityRequest` carries them

**What it costs to remove**: low. These are opaque UUIDs/strings carried
through and serialized. The crate has zero business logic keyed on their
semantic meaning. Replace with `&str` / `Uuid` / a generic
`OpaqueId(String)` wrapper.

### `agent-sandbox-api` — request/response contract

**Imported types**: `ExecutionRequest`, `ExecutionResponse`, `EffectClass`,
`SandboxError`, `SandboxBackend` trait, `SandboxExecutionAuthority`,
`SandboxBackendIdentity`.

**Coupling characterization**:
- `ExecutionRequest` / `ExecutionResponse` are the boundary — the crate
  destructures them at `backend.rs:292-375` and pulls out `workspace_id`,
  `run_id`, `effect_class`. **Generic** in shape, **predecessor-shaped** in
  fields.
- `EffectClass::ReadOnly | Mutating` is a generic enum — gates writeback
  at `backend.rs:292-375`. Trivially portable.
- `SandboxError` is the error shape returned by the trait. The crate maps
  internal `FirecrackerError` variants onto it. Generic in concept, but the
  specific variant set carries some predecessor framing.
- `SandboxExecutionAuthority` (lease epoch, owner_node_id, etc.) is
  **predecessor-specific**: it's a control-plane lease token. Used in
  `backend.rs:39, 369-375` to gate writeback. **Already behind a hook**:
  `FirecrackerWritebackAuthority` trait at `backend.rs:66-71` with a
  permissive default at `backend.rs:73-84`. Removing the default and
  letting the caller plug in is the move.

### `agent-guest-proto` — wire types for vsock

**Imported types**: `GuestRequest`, `GuestResponse`,
`GuestWorkspacePolicy`, serialization helpers.

**Where used**: `vsock.rs:2, lifecycle.rs:5, backend.rs:5, 139, 147, 182`.

**Coupling characterization**: The crate transports `GuestRequest` blobs
to the guest and parses `GuestResponse` blobs back. It does not interpret
the policy or the tool name — it's a courier. **Mostly generic**, but the
struct fields (workspace_id, run_id, tool_name, allowed_tools,
idempotency_key, effect_class) are predecessor's tool-execution shape, so a
generic CLI wants a slimmer envelope.

### `agent-domain` — `CapabilityClass`

**Imported type**: `CapabilityClass` enum (FileSystem, Shell, Network,
Search, Custom).

**Where used**: `network.rs:155-169`, exactly one function:
`resolve_vm_network_mode()`. Tests for `Network` capability and chooses
`OutboundNat` vs `NoEgress`.

**Cost to remove**: ~15 lines. Replace with a `bool` parameter
("request_network").

## Predecessor concepts woven into the lifecycle

A grep of the crate for predecessor-specific tokens. None of these *drive*
control flow — they're metadata or already-parameterized policy hooks.

| Concept | Where | Coupling depth |
|---|---|---|
| `workspace_id` / `run_id` | Snapshot manifests `snapshot.rs:57-58`, jailer plans `jailer.rs:90-91`, network state `network.rs:115-116`, triage bundles | **Identity carriers only**, never drive lifecycle branches |
| `SandboxExecutionAuthority` | `backend.rs:369-375` | **Already behind a trait hook** (`FirecrackerWritebackAuthority`). Default impl is permissive. Just need a different default at the new home. |
| `CapabilityClass::Network` | `network.rs:156-169` | **One 15-line resolver**. Replace with `bool`. |
| `EffectClass::ReadOnly` writeback gate | `backend.rs:292-375` | Generic enum, lightly opinionated semantics. Keep it. |
| JIT credentials | None inside this crate | Zero coupling. They live above. |
| `WorkspaceCommand::Drain/Evict` | None inside this crate | Zero coupling. README mentions interaction with shared command surfaces but the crate doesn't import them. |
| Control-plane authority | None | Zero coupling. |
| Checkpointing | None directly | Snapshot module references but is unwired. |

The README says "Does Not Own: guest planning logic, canonical semantics,
raw workspace policy interpretation after `VmNetworkMode` is resolved,
guest transport protocol redesign, ..." (lines 105-121). That stance is
honored in the code: the crate is genuinely a backend, not a policy
owner.

## Pure VM mechanics: what's already portable

Roughly 70% of working code is mechanism that has nothing predecessor-specific.

| Module | LOC | What it does |
|---|---|---|
| `foundation.rs` | 2,475 | Binary discovery, host preflight, privileged-command runner |
| `boot.rs` | 1,226 | Pre-boot artifact wiring, machine-config, drive setup |
| `lifecycle.rs` | 1,717 | Create/start/stop/delete state machine |
| `client.rs` | 896 | Firecracker UDS REST API client |
| `vsock.rs` | 410 | Vsock device config, ready-marker probe |
| `storage.rs` | 992 | ext4 image cloning, scratch image, admissibility scan, writeback extraction |
| `jailer.rs` | 1,457 | Jailer chroot, UID/GID drop, asset binding plan |
| `cgroup.rs` | 278 | Cgroup v2 subtree creation, pid assignment |
| `network.rs` | 3,669 | Bridge/tap/MAC/IP allocation, iptables NAT, DNS injection |

Sum: **13,120 LOC** of clearly-portable mechanism (excluding most of
the test-only inline code).

## Public API of the crate (`lib.rs`)

`/tank/projects/predecessor/crates/sandbox/agent-sandbox-firecracker/src/lib.rs`
re-exports across lines 38-116:

- **Executable interfaces**: `FirecrackerBackend`, launch/boot functions,
  jailer materialization, network resolution, snapshot/restore helpers
- **Configuration**: `FirecrackerBackendConfig`, `FirecrackerPaths`,
  `DiscoveryConfig`, `MinimalVmConfig`, `MinimalBootRequest`
- **Network policy**: `WorkspaceNetworkPolicy`, `VmNetworkMode`,
  `OutboundNatConfig`
- **Writeback hook**: `FirecrackerWritebackAuthority` (trait),
  `FirecrackerWritebackAuthorityRequest`,
  `FirecrackerWritebackAuthorizationDecision`
- **Observability**: probe snapshot, health views, metrics, diagnostics
  triage helpers
- **Storage**: `PreparedVmStorage`, `DriveConfig`
- **State schemas**: `JailerPlanIdentity`, `PreparedJailerPlan`,
  `JailerRuntimeState`, `SnapshotManifest`, `NetworkState` records

A new home for the lib re-exports ~80% of this verbatim. The portable
items are the configs and state schemas; the only directly predecessor-touching
public type is `FirecrackerWritebackAuthorityRequest` (which references
agent-ids/sandbox-api types).

## The "writeback" model

This appears prominently in the README and is worth a precise gloss
because it's the largest non-mechanical opinion in the crate.

**What it is**: the host-side atomic swap of guest workspace mutations
back into the host directory tree after a VM stops cleanly. Steps:

1. Pre-boot: prepare an ext4 scratch image, hydrate it from the host
   workspace
2. Guest mounts that image at `/var/lib/predecessor/workspace`, executes,
   modifies files
3. Post-stop:
   - `e2fsck` repairs the journal
   - `debugfs` extracts the modified file set
   - Admissibility scan rejects symlinks, special files, non-regular
     entries
   - Staged tree is atomically swapped into the host workspace
   - Rollback on any failure

**Is this intrinsic to running a Firecracker VM?** **No.** Firecracker
itself doesn't know writeback exists. It's a predecessor-specific opinion
about how host workspaces sync with guest mutations. A generic sandbox
could use snapshots, NFS mounts, virtiofs, or copy-on-write. The
admissibility model (only regular files and dirs survive writeback) is
also opinionated.

**Decoupling it**: parameterize the post-stop operation. The crate can
expose `PreparedVmStorage` (the scratch image) and let the caller decide
what to do with it after stop. The default impl can still ship a "swap
back into host workspace" implementation; the trait hook lets a CLI
caller pick "drop the image", "save it as a tarball", "mount it into
host", etc.

## The numbers

- **Strictly predecessor-specific**: ~5% of the crate (workflow authority
  default, capability resolution, some error variants, snapshot/blank-pool
  modules that are predecessor-shaped even though unwired)
- **Mildly predecessor-shaped (carries metadata, no logic)**: ~25%
  (request/response routing, ID threading, observability bundle field
  names)
- **Pure mechanics**: ~70% (lifecycle, network, jailer, cgroup, storage,
  client, vsock, foundation)

The extraction looks like *renaming* and *parameterizing*, not
*rewriting*.
