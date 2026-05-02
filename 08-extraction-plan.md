# 08 — Extraction plan (phased)

## Repository structure (target)

```
m80/
├── Cargo.toml                              # workspace root
├── README.md
├── LICENSE
├── crates/
│   ├── m80-core/                           # the library (host side)
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── error.rs
│   │   │   ├── config.rs                   # ← from foundation.rs (config types)
│   │   │   ├── preflight.rs                # ← from foundation.rs (host checks)
│   │   │   ├── client.rs                   # ← client.rs verbatim (rename types)
│   │   │   ├── vsock.rs                    # ← vsock.rs (rename ready marker)
│   │   │   ├── storage/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── image.rs                # ← from storage.rs (rootfs clone)
│   │   │   │   ├── scratch.rs              # ← from storage.rs (scratch image)
│   │   │   │   └── writeback.rs            # ← from storage.rs (admissibility + extract)
│   │   │   ├── boot.rs                     # ← boot.rs (rename predecessor refs)
│   │   │   ├── lifecycle/
│   │   │   │   ├── mod.rs                  # public Sandbox trait
│   │   │   │   ├── create.rs               # ← from lifecycle.rs
│   │   │   │   ├── start.rs                # ← from lifecycle.rs
│   │   │   │   ├── exec.rs                 # ← new (was inside backend.rs)
│   │   │   │   ├── stop.rs                 # ← from lifecycle.rs
│   │   │   │   └── delete.rs               # ← from lifecycle.rs
│   │   │   ├── jailer.rs                   # ← jailer.rs (rename paths)
│   │   │   └── cgroup.rs                   # ← cgroup.rs (feature-gated)
│   │   ├── tests/
│   │   │   ├── minimal_boot.rs
│   │   │   ├── guest_control.rs
│   │   │   └── lifecycle_e2e.rs
│   │   └── Cargo.toml
│   │
│   ├── m80-proto/                          # vsock wire types
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── envelope.rs                 # M80Request / M80Response
│   │   │   └── version.rs
│   │   └── Cargo.toml
│   │
│   ├── m80-guestd-lib/                     # guest daemon library
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── dispatch.rs                 # generic exec
│   │   │   ├── transport.rs                # vsock listener
│   │   │   └── workspace.rs                # bind-mount aliasing
│   │   └── Cargo.toml
│   │
│   ├── m80-cli/                            # the CLI binary
│   │   ├── src/
│   │   │   ├── main.rs
│   │   │   ├── commands/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── run.rs
│   │   │   │   ├── shell.rs
│   │   │   │   ├── exec.rs
│   │   │   │   ├── ls.rs
│   │   │   │   ├── stop.rs
│   │   │   │   ├── logs.rs
│   │   │   │   ├── prepare_image.rs
│   │   │   │   └── preflight.rs
│   │   │   └── output.rs
│   │   └── Cargo.toml
│   │
│   └── m80-network/                        # OutboundNat (v0.2)
│       └── ...
│
├── services/
│   └── m80-guestd/                         # guest daemon binary
│       ├── src/main.rs
│       └── Cargo.toml
│
├── infra/
│   ├── prepare-image.sh                    # ← prepare-guestd-image.sh ported
│   ├── provision-host.sh                   # ← provision-guest.sh ported
│   ├── lima/
│   │   └── m80-dev.yaml                    # ← predecessor-fc-dev.yaml ported
│   └── systemd/
│       ├── m80-guestd.service
│       └── var-lib-m80-workspace.mount
│
├── scripts/
│   ├── m80-preflight.sh                    # ← firecracker-preflight.sh ported
│   └── m80-dev-env.sh                      # ← firecracker-dev-env.sh ported
│
└── docs/
    ├── architecture.md
    ├── runbook.md
    └── examples/
```

## Phase 1: m80 v0.1 (~2-3 weeks)

### Scope

- Core library: `m80-core`, `m80-proto`, `m80-guestd-lib`
- Guest daemon: `m80-guestd` (services binary)
- CLI: `m80-cli` (top-level `m80` command)
- Image build: `infra/prepare-image.sh` + manifest schema
- Host preflight: `scripts/m80-preflight.sh`
- Lima dev wrapper: `scripts/m80-dev-env.sh` + `infra/lima/m80-dev.yaml`
- Tests: minimal-boot, guest-control, lifecycle smoke
- Linux/KVM only, single-VM at a time, NoEgress only
- No diagnostics, no scrape, no probe, no readiness, no snapshot, no
  blank-pool, no OutboundNat

### Order of operations

#### Day 1-2: bootstrap

1. Create `m80/` repo with workspace `Cargo.toml`
2. Copy `agent-sandbox-firecracker/Cargo.toml` → `m80-core/Cargo.toml`,
   strip predecessor deps, add new ones (uuid, anyhow, etc.)
3. Copy `agent-guest-proto` → `m80-proto`, replace types
4. Define `m80-proto::M80Request` and `M80Response` (from
   02-sandbox-api-and-guest-proto.md)
5. CI scaffolding: GitHub Actions on a Linux/KVM runner (or
   self-hosted)

#### Day 3-5: extract foundation, client, vsock

6. Copy `foundation.rs` → split into `config.rs` (types) and
   `preflight.rs` (checks)
7. Copy `client.rs` verbatim, rename internal references
8. Copy `vsock.rs`, rename `GUESTD_READY` → `M80_READY`
9. Get all three compiling against `m80-proto`
10. Port the foundation_tests.rs that survive the rewrite

#### Day 6-8: extract boot and lifecycle

11. Copy `boot.rs`, replace `WorkspaceId`/`RunId` with opaque string IDs
12. Copy `lifecycle.rs`, drop snapshot imports, drop predecessor-shaped
    diagnostic recording
13. Define new `Sandbox` trait in `m80-core::lifecycle::mod.rs`:

    ```rust
    pub trait Sandbox {
        fn create(&self, config: VmConfig) -> Result<VmHandle>;
        fn start(&self, vm: &VmHandle) -> Result<ReadyVm>;
        fn exec(&self, vm: &ReadyVm, cmd: ExecRequest) -> Result<ExecResponse>;
        fn stop(&self, vm: ReadyVm, opts: StopOptions) -> Result<StoppedVm>;
        fn delete(&self, vm: StoppedVm) -> Result<()>;
    }
    ```

14. Implement `Sandbox for FirecrackerSandbox`
15. Port `lifecycle_tests.rs` minimal-boot + guest-control tests

#### Day 9-10: storage and writeback

16. Copy `storage.rs`, simplify writeback into "extract scratch image"
    + optional "atomic swap to dir"
17. Make writeback a CLI flag, not a policy
18. Tests: hydrate, exec, extract

#### Day 11-12: jailer (optional for v0.1)

19. Copy `jailer.rs`, rename predecessor paths
20. Behind a `--jailed` flag in the CLI; default is no-jailer for
    bring-up
21. Cgroup behind `--cgroup-v2` flag

#### Day 13-15: guest daemon

22. New crate `m80-guestd-lib`: vsock listener, generic exec, workspace
    bind-mount
23. New binary `services/m80-guestd`: trivial wrapper around
    `m80-guestd-lib`
24. systemd unit `m80-guestd.service`
25. Systemd unit `var-lib-m80-workspace.mount`

#### Day 16-18: image build pipeline

26. Port `prepare-guestd-image.sh` → `infra/prepare-image.sh`
27. Drop the `npm`/`pip` paranoia
28. Drop the `python3`/`nodejs` requirement (add as opt-in via flag)
29. Manifest schema v1: same shape, rename fields
30. Test: build image, mount, verify, sha256

#### Day 19-21: CLI

31. `m80 preflight` — runs the host check
32. `m80 prepare-image` — runs the image build
33. `m80 run --image PATH --workspace DIR -- CMD ARGS` — one-shot
34. `m80 shell --image PATH` — interactive vsock shell
35. `m80 exec VM-ID -- CMD` — exec into a running VM
36. `m80 ls` — list active VMs in run-root
37. `m80 stop VM-ID` — graceful stop
38. `m80 logs VM-ID` — tail the per-VM log

#### Day 22-25: tests, docs, CI

39. End-to-end smoke test on Linux/KVM CI runner
40. README with one-page quickstart
41. `docs/runbook.md` with troubleshooting
42. `docs/architecture.md` with crate map
43. Tag v0.1.0

## Phase 2: m80 v0.2 (~2-3 weeks)

### Scope

- `m80-network` crate with full OutboundNat
- Optional observability: diagnostics, probe, health (single small
  crate)
- Real snapshot/restore via Firecracker `/snapshot/load`
- Multi-VM orchestration in CLI

### Order

#### Week 1: OutboundNat

44. New crate `m80-network`: address allocation, bridge/tap setup, NAT
    policy, teardown, recovery
45. Port `network.rs` carefully (10-15 days estimated, see file 06)
46. Hide behind `--network outbound-nat` CLI flag; default is no-egress

#### Week 2: observability

47. New crate `m80-observability` (or feature-gate inside `m80-core`)
48. Port `diagnostics.rs` (slimmed)
49. Port `probe.rs` + `health.rs` for `m80 ls --status`
50. Skip `scrape.rs`, `readiness.rs`, `ops_metrics.rs` for now

#### Week 3: snapshot + multi-VM

51. Real snapshot/restore (not the unwired stub)
52. CLI: `m80 snapshot VM-ID --to FILE`, `m80 restore --from FILE`
53. CLI: `m80 run --background` (don't block on stop)
54. Tag v0.2.0

## Phase 3: predecessor cutover (~2-4 days)

### Scope

Replace `agent-sandbox-firecracker` inside predecessor with a thin adapter
that uses m80.

### Order

55. New crate `crates/sandbox/m80-adapter`:

    ```rust
    pub struct PredecessorM80Backend {
        m80: m80_core::FirecrackerSandbox,
        writeback_authority: Arc<dyn FirecrackerWritebackAuthority>,
        placement: Arc<PlacementState>,
    }

    #[async_trait]
    impl SandboxBackend for PredecessorM80Backend {
        async fn execute(&self, request: ExecutionRequest)
            -> Result<ExecutionResponse, SandboxError>
        {
            let vm = self.m80.create(...)?;
            let ready = self.m80.start(&vm)?;
            let m80_request = translate_predecessor_to_m80(request);
            let m80_response = self.m80.exec(&ready, m80_request)?;
            let stopped = self.m80.stop(ready, ...)?;
            // writeback authority gate, then writeback
            // placement_state.observe_materialized(...)
            self.m80.delete(stopped)?;
            Ok(translate_m80_to_predecessor(m80_response))
        }
    }
    ```

56. The adapter still owns:
    - Translation `ExecutionRequest ↔ M80Request`
    - Translation `M80Response ↔ ExecutionResponse`
    - Writeback authority hook (was inside firecracker crate)
    - Placement-state observation (was inside `TrackingFirecrackerBackend`)
    - The predecessor wire format on the guest side
      (the m80 guest daemon can be replaced with the predecessor `guestd-rs`
      that already speaks `agent-guest-proto`; or predecessor ships its own
      guest daemon image and m80 just provides the host side)

57. Update `worker-rs/src/executor_factory.rs` to construct
    `PredecessorM80Backend` instead of `FirecrackerBackend`
58. Update `sandbox-executor-rs/src/lib.rs` similarly
59. Delete `crates/sandbox/agent-sandbox-firecracker/`
60. Run the full test suite; fix anything that breaks
61. Update `docs/crate-map.md`
62. Commit, file PR

## Critical decisions to make before starting

1. **Does m80 ship its own guest daemon, or reuse predecessor's?** Two
   options:
   - **Own daemon**: m80 ships `m80-guestd` with generic exec. predecessor's
     adapter ships its own image with `guestd-rs`. Cleaner but two
     guest daemons to maintain.
   - **Generic guest, predecessor adapter on top**: m80 ships `m80-guestd`
     and `M80Request`. predecessor's adapter wraps `M80Request` with its
     own envelope or builds a separate guest daemon.

   **Recommendation**: m80 ships its own daemon. predecessor keeps
   `guestd-rs`. The adapter on the host side translates
   `ExecutionRequest → exec("/usr/local/bin/guestd-rs-shim", args)`,
   where the shim is a thin in-VM helper. Or, more cleanly, predecessor's
   `guestd-rs` becomes the m80 guest binary in predecessor's image; m80's
   public image uses `m80-guestd`.

2. **Repo: monorepo or separate?** m80 starts as a separate repo. If
   coupling becomes painful, switch to git submodule or path-dep later.

3. **License**: pick one (MIT? Apache-2.0? both?). Document.

4. **Versioning**: SemVer with public-API stability promise after v1.0.
   v0.x is unstable.

5. **Distribution**: crates.io for the library, GitHub releases for the
   binary. No package managers in v0.1.
