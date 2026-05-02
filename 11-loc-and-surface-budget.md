# 11 — LOC and surface budget

## Source LOC by module (verified `wc -l`)

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

Total: 27,623 LOC
```

## Test LOC

Inline `*_tests.rs`:
- diagnostics_tests.rs: 739
- foundation_tests.rs: 1,403
- jailer_tests.rs: 1,305
- lifecycle_tests.rs: 2,789

Sum: 6,236 LOC of inline tests.

External tests (`tests/`):
- backend_conformance.rs
- guest_control.rs
- minimal_boot.rs
- no_egress.rs
- outbound_nat.rs
- workspace_writeback.rs

(Sizes not measured but estimated 1,500-2,500 LOC total.)

**Source-only (no tests)**: 27,623 - 6,236 = **21,387 LOC**.

## v0.1 budget (no `network.rs`, no observability tail, no
unwired modules)

Keep in v0.1:
```
1281 backend.rs        → rewrite to ~600 LOC (split into create/start/exec/stop/delete)
1226 boot.rs           → port ~1,200 LOC
 896 client.rs         → port ~890 LOC
 949 errors.rs         → trim to ~600 LOC
2475 foundation.rs     → split into config.rs (~500) + preflight.rs (~700) + helpers (~1,000) = ~2,200 LOC
1457 jailer.rs         → port ~1,400 LOC
 278 cgroup.rs         → port ~270 LOC (feature-gated)
 129 lib.rs            → rewrite ~150 LOC
1717 lifecycle.rs      → port ~1,400 LOC (drop snapshot imports, drop predecessor diag)
 992 storage.rs        → port ~900 LOC (simplify writeback)
 410 vsock.rs          → port ~400 LOC

Sum (m80-core source): ~10,010 LOC
```

Plus:
- `m80-proto` (new): ~400 LOC
- `m80-guestd-lib` (rewrite): ~800 LOC
- `services/m80-guestd` (rewrite): ~300 LOC
- `m80-cli` (new): ~1,500 LOC

**v0.1 total: ~13,000 LOC** of new/ported source code.

Plus tests: aim for ~50% of source = ~6,500 LOC.

## v0.2 additions

```
3669 network.rs        → port ~3,500 LOC (slim predecessor refs)
 997 snapshot.rs       → rewrite real impl ~800 LOC (don't copy unwired draft)
1164 diagnostics.rs    → slim port ~700 LOC
 549 probe.rs          → port ~520 LOC
 285 health.rs         → port ~280 LOC

Sum: ~5,800 LOC
```

**v0.2 total: ~18,800 LOC** of source code (cumulative).

## Dropped entirely (v0.1 + v0.2)

```
1412 blank_pool.rs     drop
 455 ops_metrics.rs    drop (or defer to v0.3+)
 729 scrape.rs         drop (or defer)
 317 readiness.rs      drop
 ~350 of errors.rs     drop predecessor-shaped variants

Sum dropped: ~3,260 LOC
```

## Public API surface

### v0.1 public types (m80-core::lib)

```rust
// Lifecycle
pub trait Sandbox { ... }
pub struct FirecrackerSandbox { ... }
impl FirecrackerSandbox {
    pub fn new(config: SandboxConfig) -> Result<Self>;
}

pub struct VmConfig { ... }
pub struct VmHandle { ... }
pub struct ReadyVm { ... }
pub struct StoppedVm { ... }

// Configs
pub struct SandboxConfig { ... }
pub struct DiscoveryConfig { ... }
pub struct VmPaths { ... }
pub struct StopOptions { ... }
pub struct ExecRequest { ... }
pub struct ExecResponse { ... }

// Errors
pub enum M80Error { ... }
pub type Result<T> = std::result::Result<T, M80Error>;

// Preflight
pub fn run_preflight(config: &PreflightConfig) -> Result<PreflightReport>;
pub struct PreflightReport { ... }

// Recovery
pub fn recover_stale_run_root(run_root: &Path) -> Result<RecoveryReport>;
pub struct RecoveryReport { ... }

// Boot identity
pub struct BootIdentity { ... }

// Network (v0.2)
pub enum VmNetworkMode { NoEgress, OutboundNat(OutboundNatConfig) }
pub struct OutboundNatConfig { ... }

// Image manifest
pub struct ImageManifest { ... }
pub fn read_manifest(path: &Path) -> Result<ImageManifest>;
pub fn validate_manifest(m: &ImageManifest, image_path: &Path) -> Result<()>;
```

### m80-proto::lib

```rust
pub const PROTOCOL_VERSION: u8 = 1;

pub struct M80Request {
    pub version: u8,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
    pub workspace_dir: Option<PathBuf>,
    pub stdin: Option<Vec<u8>>,
    pub timeout_ms: u64,
}

pub struct M80Response {
    pub version: u8,
    pub status: ExitStatus,
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timing: M80Timing,
}

pub enum ExitStatus { Completed, TimedOut, Cancelled, Failed }
pub struct M80Timing {
    pub started_at: SystemTime,
    pub stopped_at: SystemTime,
    pub spawn_ms: u64,
    pub run_ms: u64,
}

pub fn write_request<W: Write>(w: &mut W, req: &M80Request) -> Result<()>;
pub fn read_request<R: BufRead>(r: &mut R) -> Result<M80Request>;
pub fn write_response<W: Write>(w: &mut W, resp: &M80Response) -> Result<()>;
pub fn read_response<R: BufRead>(r: &mut R) -> Result<M80Response>;
```

### m80-cli (binary)

Subcommands: `preflight`, `prepare-image`, `run`, `shell`, `exec`,
`ls`, `stop`, `logs`, `inspect`, `cleanup`, `config`.

## Dependency closure (v0.1 m80-core)

Direct deps:
```
serde, serde_json
thiserror
tokio
tracing
tempfile
fs2
sha2
rustix (process)
async-trait
chrono
uuid              (replaces agent-ids)
m80-proto         (workspace)
```

11 deps. All portable. None pull in agent-platform concerns.

Removed compared to current crate:
```
agent-domain         dropped (CapabilityClass replaced with bool)
agent-ids            replaced (uuid)
agent-sandbox-api    dropped (own trait)
agent-guest-proto    replaced (m80-proto)
```

## Comparison: predecessor crate sizes

For reference, the firecracker crate is the **largest** sandbox-related
crate in predecessor:

```
agent-sandbox-firecracker:  27,623 LOC (with tests)
agent-sandbox-container:    ~3,000 LOC (estimated)
agent-sandbox-local:        ~2,000 LOC (estimated)
agent-sandbox-api:          ~800 LOC (estimated)
agent-guest-proto:          ~600 LOC (estimated)
agent-guestd-lib:           ~1,100 LOC
services/guestd-rs:         ~1,500 LOC
agent-tool-executor:        ~2,000 LOC (estimated)
agent-sandbox-tool-catalog: ~500 LOC (estimated)
```

m80 v0.1 ships ~13,000 LOC of source — about half of what's currently
in the firecracker crate alone. The reduction comes from:
- Dropping unwired modules (snapshot, blank_pool): -2,400 LOC
- Dropping observability tail (probe, health, scrape, readiness,
  ops_metrics): -2,300 LOC
- Slimming predecessor-shaped error variants: -350 LOC
- Trimming the writeback model: -200 LOC
- Dropping `network.rs` for v0.1: -3,500 LOC

Net: a focused, ship-able library at half the surface area. v0.2 adds
the network and observability slices, growing to ~18,800 LOC.
