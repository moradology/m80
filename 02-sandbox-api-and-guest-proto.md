# 02 — Abstraction layer: `SandboxBackend` trait and guest protocol

## Crates examined

- `/tank/projects/predecessor/crates/sandbox/agent-sandbox-api/`
- `/tank/projects/predecessor/crates/sandbox/agent-guest-proto/`
- `/tank/projects/predecessor/crates/sandbox/agent-sandbox-tool-catalog/`
- `/tank/projects/predecessor/crates/sandbox/agent-tool-executor/`

## The trait itself

From `agent-sandbox-api/src/backend.rs:111-134`:

```rust
#[async_trait]
pub trait SandboxBackend: Send + Sync {
    fn observability_identity(&self) -> SandboxBackendIdentity {
        SandboxBackendIdentity::local_direct()
    }

    async fn execute(
        &self,
        request: ExecutionRequest,
    ) -> Result<ExecutionResponse, SandboxError>;
}
```

The trait is intentionally minimal: an identity hook for telemetry
provenance, and a single `execute` method. There is **no create/start/stop
in the trait** — the backend hides VM lifecycle entirely behind a single
RPC-shaped call. That's a deliberate design choice and it works for
predecessor's per-call sandboxing model.

For an m80 library, this is the wrong shape. m80 wants:
- `prepare_image()` → image ID
- `create(image_id, config)` → VM handle
- `start(vm)` → ready
- `exec(vm, command)` → result, possibly streamed
- `stop(vm)` → clean
- `delete(vm)` → freed

m80 should ship its own trait. The predecessor `SandboxBackend` becomes a
*consumer* of m80, glued together in `m80-adapter`.

## ExecutionRequest shape

From `agent-sandbox-api/src/lib.rs:115-148`:

```
workspace_id
run_id
tool_call_id
correlation_id
idempotency_key
authority: Option<SandboxExecutionAuthority>
tool_name: String
arguments: String                  // JSON
workspace_root: PathBuf
cwd: Option<PathBuf>
effect_class: EffectClass          // ReadOnly | Mutating
timeout_ms: u64
artifact_capture_hints: Vec<ArtifactCaptureHint>
```

Six identity fields, an authority lease, a named tool with JSON args,
a workspace root, an effect class, a timeout, and capture hints. This is
"agent platform invokes named tool in sandbox" — not "user runs command".

For m80 a simpler envelope works:

```
program: PathBuf
args: Vec<String>
cwd: Option<PathBuf>
env: Vec<(String, String)>
workspace_dir: Option<PathBuf>      // bind-mounted into VM
stdin: Option<Bytes>
timeout: Duration
```

## ExecutionResponse shape

From `agent-sandbox-api/src/lib.rs:200+`:

```
status: Completed | TimedOut | Cancelled | Failed
stdout: Inline(Vec<u8>) | Externalized { uri, sha256, size_bytes }
stderr: Inline(Vec<u8>) | Externalized { uri, sha256, size_bytes }
exit_code: Option<i32>
captured_artifacts: Vec<...>
error_class: Option<...>
timing: ...
```

Most of this is portable. The Inline-vs-Externalized split is interesting
prior art for streaming/large outputs — keep it as an idea but ship Inline
only in v0.1.

## SandboxExecutionAuthority

From `agent-sandbox-api/src/types.rs`:

```
owner_node_id
lease_epoch
owner_worker_instance_id
live_attempt_id
authority_deadline_epoch_ms
```

Pure control-plane leasing semantics. Not relevant to a CLI user. Leave
it in predecessor; the adapter passes it through to the writeback hook only.

## Backend implementations and what they prove

### LocalSandboxBackend

`/tank/projects/predecessor/crates/sandbox/agent-sandbox-local/`
(`executor.rs:73-149`). Direct host execution, no isolation. Hydrates
workspace confinement, dispatches to local tool handlers (Bash, Exec,
ReadFile, etc.), captures artifacts. **Lean** — no network, no spawning
beyond the tool itself.

### SimpleBackend

`/tank/projects/predecessor/crates/sandbox/agent-sandbox-local/src/simple.rs:49-75+`.
In-process tool dispatch through `ToolRegistry`. No filesystem, no network.
Synchronous, in-memory. Used in tests. **Demonstrates the trait works for
in-process semantics.**

### ContainerBackend

`/tank/projects/predecessor/crates/sandbox/agent-sandbox-container/src/lib.rs:1-150+`.
Spawns a container, runs `guestd-rs` inside, speaks `agent-guest-proto`
over stdio. Configurable runtime, image, env, lifecycle timeouts.
**Demonstrates the protocol works across a process boundary.**

### FirecrackerBackend

`/tank/projects/predecessor/crates/sandbox/agent-sandbox-firecracker/src/backend.rs`.
Spawns a microVM via the firecracker binary, speaks `agent-guest-proto`
over vsock. **Demonstrates the protocol works across a VM boundary.**

The trait has been forced to generalize across three subprocess models.
That's why it's clean. The blockage isn't the trait — it's that every use
site assumes predecessor's metadata model.

## Guest protocol (`agent-guest-proto`)

From `envelope.rs:36-98`, schema version 1:

```rust
pub struct GuestRequest {
    pub version: u8,                    // = 1
    pub workspace_id: WorkspaceId,
    pub run_id: RunId,
    pub tool_call_id: ToolCallId,
    pub correlation_id: CorrelationId,
    pub idempotency_key: IdempotencyKey,
    pub tool_name: String,
    pub arguments: String,              // JSON
    pub workspace_root: PathBuf,
    pub cwd: Option<PathBuf>,
    pub effect_class: EffectClass,
    pub timeout_ms: u64,
    pub artifact_capture_hints: Vec<ArtifactCaptureHint>,
    pub workspace_policy: GuestWorkspacePolicy,
}

pub struct GuestResponse {
    pub version: u8,                    // = 1
    pub tool_call_id: ToolCallId,
    pub status: ExecutionStatus,
    pub stdout: ResponseBytes,
    pub stderr: ResponseBytes,
    pub exit_code: Option<i32>,
    pub captured_artifacts: Vec<...>,
    pub error_class: Option<...>,
    pub timing: ExecutionTiming,
}

pub struct GuestWorkspacePolicy {
    pub read_only: bool,
    pub allowed_tools: Option<Vec<String>>,
}
```

NDJSON over vsock. Max 4 MiB per frame (`envelope.rs:22`).

**Issue for extraction**: this is a 1:1 mirror of `ExecutionRequest`. It
forces the guest to know about `tool_name`, `allowed_tools`,
`idempotency_key`, etc. m80 wants a slimmer envelope.

**Resolution**: ship `m80-proto` as a v1 protocol with:

```rust
pub struct M80Request {
    pub version: u8,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
    pub stdin: Option<Vec<u8>>,
    pub workspace_dir: Option<PathBuf>,
    pub timeout_ms: u64,
}

pub struct M80Response {
    pub version: u8,
    pub status: ExitStatus,        // Completed | TimedOut | Cancelled | Failed
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timing: M80Timing,
}
```

predecessor's adapter retains `agent-guest-proto` and bridges `GuestRequest →
M80Request` on the host side. The predecessor guest daemon stays on
`GuestRequest`; the m80 guest daemon (a different binary) speaks
`M80Request`.

## Tool catalog (`agent-sandbox-tool-catalog`)

From `lib.rs:14-23`:

```rust
pub enum LocalTool {
    Bash,
    Exec,
    ReadFile,
    WriteFile,
    Build,
    Test,
}
```

Six fixed tools, each with a name, description, JSON parameter schema,
and capability_class (`Shell` or `FileSystem`). **This belongs to predecessor**,
not m80. m80 ships "run an arbitrary command"; the curated catalog is an
agent-platform concern.

## Tool executor (`agent-tool-executor`)

`dispatch.rs:1-100`. Implements `ToolExecutor` trait. Owns:
- Host-side `WorkspacePolicy` validation (read_only flag, allowed_tools)
- Tool registry validation (does `tool_name` exist?)
- Dispatch into `SandboxBackend::execute`

This is the layer that combines "named tool with policy" → "execute call".
**Stays in predecessor.** m80 does not have a tool model.

## Verdict

The `SandboxBackend` trait is a real, pressure-tested abstraction, but
it's the wrong abstraction for m80 because it's fundamentally
"agent-shaped". m80 wants explicit lifecycle (create/start/exec/stop)
rather than collapsed-RPC.

**Plan**:
1. m80 ships its own `Sandbox` trait with explicit lifecycle methods
2. m80 ships its own `m80-proto` wire format (slim, generic-exec)
3. m80 ships its own guest daemon (`m80-guestd`)
4. predecessor keeps `SandboxBackend`, `ExecutionRequest`,
   `agent-guest-proto`, `agent-sandbox-tool-catalog`, `agent-tool-executor`
   intact
5. `m80-adapter` translates between the two, hosts the writeback
   authority hook, and registers the placement-state callback
