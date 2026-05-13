# Changelog

All notable changes to m80 are documented here. Format roughly follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Fixed — `m80-jailer` bind-remount regression (caught running the new bench)

- `crates/m80-jailer/src/plan.rs`: restored `MS_BIND` in
  `bind_remount_flags()`. An earlier audit pass (`m80-l020n.9`) removed
  it based on the claim that "the kernel ignores `MS_BIND` on remount" —
  wrong. For a bind-mount, `MS_BIND|MS_REMOUNT` is required to
  disambiguate which mount the remount targets; without it the second
  `mount()` call fails with `EBUSY` after the initial bind succeeds.
  Every `m80 run` had been broken between the audit-sweep landing and
  this fix. Caught only when actually running the new bench against
  real KVM (no unit/mock test could see this — it's a kernel state
  issue).

### Added — perf bench harness foundation (m80-ekbk, B0 + B1-B10 scaffolds)

- `scripts/bench-cold-launch.sh`: extended percentiles (P50/P75/P90/P95/
  P99/P99.9/max), histograms, bootstrap-95% CI on the median, 2σ outlier
  flagging; new env vars `WARMUP`, `SWEEP`/`SWEEP_VALUES`, `CONCURRENT`,
  `TASKSET`, `CPU_GOVERNOR`, `PHASE_JSONL`; flags `--cold-isolation`,
  `--dry-run`, `--help`.
- `scripts/bench-extras.sh` (NEW): per-sub-epic orchestrators
  (`--throughput` B2, `--memory` B4, `--teardown` B8, `--boot-decomp`
  B1, `--long-tail` B10, `--density` B3).
- `scripts/bench-summary.py`: added `compute_throughput`,
  `compute_memory`, `compute_teardown`, `compute_sweep`,
  `compute_concurrent`, `bootstrap_ci_median`, `_log_histogram`,
  `_count_outliers_2sigma`. 46 inline unit tests.
- `scripts/test-bench-harness.sh` (NEW): 25 plumbing tests on help text,
  `--dry-run`, env-var surfacing, bench-extras mode catalog.
- `docs/perf/bench-harness.md` (NEW): full knob/flag/sub-mode reference.
- `crates/m80-firecracker/benches/baseline.json`: real-KVM baseline from
  N=200 minimal/idle run on 2026-05-13 — P50=1728ms, P95=1733ms,
  P99=1736ms, P99.9=1736ms, max=1744ms, outliers=2, P50 CI95=[1728,1729].
- `docs/perf/cold-launch.md`: added "Tail-latency baseline" section with
  the N=200 numbers + per-phase tail table + guest-side boot milestones.

### Fixed — `scripts/bench-cold-launch.sh` sudo escape

- The `m80_invoke` shell function (added for optional `TASKSET`
  wrapping) was placed after `sudo` in the launch command. sudo can't
  see shell functions, so every launch errored with `m80_invoke:
  command not found`. Replaced with a `TASKSET_PREFIX` array that
  expands correctly under `sudo ENV=VAL ${TASKSET_PREFIX[@]} cmd`.
  Found by stracing the bench against the host's real m80 binary —
  the prior mock-based "e2e" had never invoked the sudo branch and
  silently masked this bug.

### Removed — mock m80 bench harness (`scripts/mock-m80.sh` + `TEST_MODE`)

- The mock binary and the `TEST_MODE` shell branches in
  `bench-cold-launch.sh` / `bench-extras.sh` are gone. They produced a
  comforting "162 closed beads + e2e all green" picture that missed
  exactly the two bugs above (kernel mount flags, sudo escape).
  Bench tests now require sudo + KVM + pre-built images; the python
  unit tests + `--help` / `--dry-run` plumbing tests are the only
  parts that run without privilege.

### Changed — audit-sweep workspace cleanup (m80-nqjm4)

- Ran a ten-iteration audit sweep (~100 parallel agents, ~150 findings). Major
  outcomes: tightened `[workspace.lints]` to warn on `unreachable_pub`,
  `map_err_ignore`, `redundant_clone`, `unwrap_used`, `string_slice`,
  `must_use_candidate`, `manual_let_else`, `wildcard_imports` and deny
  `dbg_macro`/`todo`/`unimplemented`/`unused_must_use`; applied clippy
  auto-fixes across 172 test suites (~80 warnings cleared); renamed
  `Limits::m80_default` → `Limits::preset` in `m80-cgroup`; deleted dead
  `Phase::Writeback`, `ExitReason::KernelPanic`, `ExitReason::OomKill`
  variants from `m80-observability`; deduped iptables helpers in
  `m80-net-outbound`; added `#[serde(deny_unknown_fields)]` to
  `CreateSnapshotConfig`, `LoadSnapshotConfig`, `JailerSocket`, and
  `ExecStatusJson`; unpinned `tempfile`/`clap`/`assert_cmd`/`indexmap` from
  `=X.Y.Z` to `^X`; added `scripts/run-e2e.sh` harness; dropped Drop-side
  ceremony from `m80-vsock::Channel`; corrected tracing levels across hotplug,
  warm-pool lease, and exec paths; guestd now sends a `Failed` error frame
  before disconnect on oversized-payload reads.
- Added `m80-jailer::JailerError::BindDestRejected` for policy rejections and
  promoted `JailedFirecracker.jailer_pid`/`firecracker_pid` to accessor methods
  (previously `pub` fields).
- Added `docs/behaviors/admission/vm-id-path-budget.md`,
  `docs/decisions/2026-05-01-validate-af-unix-path-budget-at-admission.md`, and
  `docs/postmortems/2026-05-08-af-unix-path-budget.md` capturing the AF_UNIX
  path-length constraint and admission-time validation design.

### Added — JoinNetns network policy mode (m80-a8g4)

- Added `NetworkPolicy::JoinNetns` as an explicit mode that places the
  Firecracker VMM process into a caller-supplied network namespace rather than
  inheriting the host default or using an outbound NAT bridge. Resolves the
  documented compromised-VMM network boundary gap.
- `m80-jailer` materializes `JoinNetns` binds and passes `--netns` to the
  Firecracker jailer. `m80-net-mode` exposes `resolve_mode` dispatch and
  updated tests pin all three policy variants.
- Added an ignored `join_netns_real_kvm` integration test suite
  (`crates/m80-firecracker/tests/join_netns_real_kvm.rs`) that boots a VM
  into a caller-created network namespace and asserts the guest NIC is
  present and the host default route is not visible.
- Added an ignored `layer1_guest_smoke_real_kvm` suite with a static C probe
  (`tests/fixtures/layer1_probe.c`) that runs inside the guest and reports
  kernel version, PID namespace depth, UID, capability mask, and visible
  mount points — establishing a Layer 1 guest-side baseline.
- Added `docs/behaviors/network-join-netns/configuration.md` capturing the
  JoinNetns policy contract.

### Added — guestd protocol hardening (m80-g0v8.12+)

- Hardened guestd cancellation crash paths: connection teardown and PTY process
  management now handle SIGKILL races without panicking PID 1.
- `m80-cgroup` gained explicit error propagation for `probe_mounts`
  (no longer silently recasts I/O errors as `UnsupportedHostMode`);
  `cleanup_orphan_subtree` returns `CgroupError::LivePids` for live-process
  conditions; `apply_limits` skips the `io.weight` write when the value equals
  the kernel default.
- `m80-jailer` and `m80-jailer-harden` gained cgroup setup hardening and
  inherited-limits propagation. Jailer privilege-drop behavior is now
  documented at `docs/behaviors/jailer/privilege-drop.md`.
- `m80-guestd` protocol hardening: `m80-proto` fileops wire conversions moved
  to `wire/conversions/fileops.rs`; `FileError` metadata path added; guestd
  `guest_log.rs` extracted for structured in-guest logging; pid-one startup
  hardened against `close().unwrap()` panics in the unwind path.
- Documented exec-privilege policy, DNS-name allowlist scope, and OOM event
  surface boundaries at `docs/behaviors/`.

### Added — jailer process-hardening coverage (m80-g0v8.11.x)

- Pinned jailer hardening inheritance coverage: the ignored defense-in-depth
  suite records that seccomp/no-new-privileges/capability-drop inheritance is
  verified through the jailer launch path (`crates/m80-jailer/tests/defense_in_depth.rs`).

### Added — prometheus exposition hardening (m80-obs)

- `m80-observability` prometheus exposition now has a dedicated test suite
  (`tests/guest_metrics_prometheus.rs`) that pins the Prometheus text format
  contract, label escaping, and counter/gauge/histogram shape for all
  exposed metric families.

### Added — OutboundNat launch wiring (m80-mx2t.4)

- `m80-firecracker` now wires `NetworkPolicy::AllowOutbound` through launch:
  bridge/TAP realization, PID-1 `m80.net.*` boot tokens, host NAT/filter policy,
  Firecracker `eth0` network-interface PUT, and owned network cleanup on launch
  failure, delete, or preserve-for-triage after stop/force-kill.

### Added — positive outbound egress E2E (m80-mx2t.2)

- Added ignored real-KVM coverage that boots `NetworkPolicy::AllowOutbound` and
  proves guest DNS resolution plus external HTTP when
  `M80_RUN_EXTERNAL_NETWORK_E2E=1` is set.

### Added — Firecracker CVE floor (m80-g0v8.11.4)

- `m80-preflight` now rejects Firecracker versions covered by the tracked
  Firecracker CVE floor before accepting any exact version pin, starting with
  AWS advisories CVE-2026-5747 and CVE-2026-1386.

### Documented — compromised-VMM network boundary (m80-g0v8.11.12)

- Documented that `NoEgress` and `AllowOutbound` are guest networking policies,
  not private network namespace guarantees for a compromised Firecracker VMM
  process, and pinned `JoinNetns` as the explicit VMM netns placement mode.

### Added — network operation attack battery (m80-g0v8.11.7)

- Added attack-runner primitives and ignored jailer harness tests for privileged
  network operations: raw sockets, packet sockets, netlink link mutation,
  nonlocal bind, and route mutation.

### Added — cgroup-enrolled attack-runner harness (m80-g0v8.11.13)

- Added a `sleep_briefly` attack-runner harness control and an ignored
  defense-in-depth test that enrolls the live jailer-launched payload in
  `m80-cgroup` with `Limits::m80_default()`.

### Added — two-tenant attack-runner harness (m80-g0v8.11.14)

- Added fixed-file peer config transport for env-cleared attack-runner launches
  plus an ignored two-tenant jailer fixture with distinct live uid/gid pairs.

### Added — cross-tenant attack battery (m80-g0v8.11.10)

- Added ignored jailer harness tests for cross-tenant read/write/list,
  peer-network-state read, peer-pid signal, and peer-run-dir bind-mount
  attempts.

### Added — resource exhaustion attack battery (m80-g0v8.11.9)

- Added ignored jailer harness tests for file-descriptor, pids, memory, and
  file-size exhaustion attempts after cgroup enrollment.

### Added — compromised-Firecracker composition test (m80-g0v8.11.11)

- Added the ignored Layer 2 capstone that runs the jailer attack batteries as a
  compromised-Firecracker simulation.

### Added — Layer-1 Firecracker config audit (m80-g0v8.11.3)

- Firecracker machine config now carries an explicit `cpu_template = T2`
  alongside `smt = false`, and tests pin the serialized REST body plus the
  preboot plan's documented device set.

### Added — attack-runner payload and jailer harness (m80-g0v8.11.1/.2)

- Added `m80-attack-runner`, a small malicious test payload with 30 stable
  attack primitives across filesystem, process, network, privilege, resource,
  and cross-tenant categories, plus the `echo_zero` harness negative control.
- The crate builds for `x86_64-unknown-linux-musl` so later jailer
  defense-in-depth tests can copy a single static payload into the jail.
- Added the ignored `m80-jailer` defense-in-depth harness that launches the
  attack runner through the official Firecracker jailer path and verifies the
  `echo_zero` negative control is observed as a successful attack.
- Added the ignored filesystem escape battery over that harness, covering
  dotdot/openat-style chroot escape attempts, proc-self-root escape, host
  sentinel read/write, and lower-layer write attempts.
- Added the ignored process/PID isolation battery, covering host PID
  status/cmdline/mountinfo observation, host-PID signal probes, and broad
  process enumeration.
- Added the ignored privilege escalation battery, covering uid/gid 0 attempts,
  retained capabilities, mount namespace creation, tmpfs mounts, and hostname
  mutation.

### Added — PID-1 outbound network configuration (m80-mx2t.4.2)

- `m80-guestd` PID-1 mode now parses bounded `m80.net.*` kernel cmdline
  tokens, configures the guest interface directly through rtnetlink, adds the
  default route, and writes `/etc/resolv.conf` after the overlay pivot.
- `m80-net-outbound` now exposes a current-image PID-1 cmdline preparation
  path that discovers admitted DNS, records configured network state, and
  returns deterministic tokens for launch wiring. The older debugfs
  networkd/resolved injection path is documented as systemd-image-only.

### Added — OutboundNat real-host entrypoints (m80-mx2t.4)

- `m80-net-outbound` now exposes real-host wrappers for bridge/TAP realize,
  guest network config injection, VM cleanup, and orphan bridge cleanup. These
  match the README's existing public-surface contract and are the entrypoints
  `m80-firecracker` needs before `AllowOutbound` can be wired through launch.

### Added — Firecracker network interface API surface (m80-2ggw.3.3)

- Added `NetworkInterfaceConfig` and `Client::put_network_interface` for
  Firecracker `PUT /network-interfaces/{iface_id}` with typed
  `NetworkInterfaceWriteFailed` error mapping.
- `m80-firecracker` preboot planning can now insert a network-interface PUT
  for an already-realized OutboundNat TAP. `AllowOutbound` remains rejected in
  phase 6 until the full OutboundNat launch wiring lands.

### Added — image-build atomicity coverage (m80-g0v8.4)

- `m80-image-build` now enters a private mount namespace before loop-mounting
  Ubuntu or Minimal output rootfs images, preventing host-visible loop mount
  leaks if the builder is killed mid-install.
- Added an ignored real-host SIGKILL regression that pauses a Minimal build
  after the loop mount, kills the process, and verifies no partial manifest,
  no leaked host mount, and a cleanable output directory.

### Added — launch failure cleanup coverage (m80-g0v8.1)

- Added an ignored real-host cgroup-create failure test that injects a
  phase_5b failure after jailer launch and verifies `FcError::Cgroup`, fake
  Firecracker process cleanup, partial run-dir removal, and admission permit
  reuse.
- Added forced-kill ambiguity coverage and behavior: an unproven SIGKILL now
  records `CleanupReleaseBlocker::ForcedKillAmbiguous` and keeps the admission
  permit held instead of silently releasing capacity.

### Added — malicious guestd test harness (m80-g0v8.12.1)

- Added `m80-guestd-malicious`, a separate test-only guest daemon artifact for
  real-KVM adversarial guest-to-host wire tests. The initial `noop` mode binds
  the normal guest vsock listener and sends the standard host readiness signal
  so later L12 leaves can add hostile frame emitters without touching
  production `m80-guestd`.
- Added the first hostile frame mode, `oversized_length`, plus an ignored
  real-KVM test that verifies the host returns
  `WireProtocolError::OversizedFrame` without allocating the announced body.
- Added `truncated_frame`, an adversarial mode that declares a frame length,
  writes a short body, closes the channel, and verifies the host returns a
  bounded disconnect-before-terminal error without a stuck reader.
- Added `unknown_variant`, an adversarial mode that emits an out-of-schema
  envelope payload field and verifies the host reports the offending field
  number instead of silently dropping the unknown protobuf data.
- Added `response_type_mismatch`, an adversarial mode that echoes the active
  request id while returning an `exec_exit` envelope with a
  `file_read_response` payload, verifying the host reports the expected and
  observed payload kinds.
- Added `bogus_request_id`, an adversarial mode that returns a valid
  `exec_exit` frame for a fabricated request id and verifies the host reports
  both expected and observed ids.
- Added `unsolicited_response`, an adversarial mode that writes a valid
  `exec_exit` frame without reading the host request and pins the current
  reject-and-teardown policy as a request-id mismatch.
- Added `unsolicited_flood` and `slowloris` adversarial modes. The real-KVM
  coverage pins flood fail-fast behavior, bounded host RSS growth, and
  protocol-level read timeout reporting for no-progress peers.

### Added — adversarial wire coverage map (m80-g0v8.12)

- Documented the real-KVM malicious-guestd coverage matrix for guest-to-host
  wire attacks, including synthetic-peer coverage, expected typed
  errors/diagnostics, and residual gaps.

### Added — wire file operations (m80-6zim)

- Added first-class wire verbs for `file_read`, `file_write`, `file_list`,
  `file_stat`, `file_remove`, and chunked write
  (`file_write_begin` / `file_write_chunk` / `file_write_commit`).
- `m80-guestd` handles file ops directly in the guest, avoiding `bash -c`,
  shell quoting, process spawn, and base64-through-stdout round trips for
  common agent file movement.
- `RunningSandbox` exposes `read_file`, `write_file`, `list_dir`,
  `stat_file`, `remove_file`, and `upload_file_chunked` wrappers that map
  guest `FileError` responses to `FcError::FileOp`.

### Added — CLI signal cancellation (m80-lt15.22)

- `m80 run` now maps SIGINT, SIGTERM, and SIGHUP received during an in-flight
  guest exec to same-connection `cancel_request` frames and exits with the
  conventional `128 + signal` code when guestd confirms cancellation.
- `m80-vsock::Channel::try_clone_sender` supports same-connection control
  frames while the main channel is blocked reading exec output.
- `RunningSandbox::exec_with_cancel` and
  `RunningSandbox::exec_streaming_with_cancel` expose cancellable buffered and
  streaming exec to host callers.
- Guestd starts execs in a fresh process group and uses bounded TERM→KILL group
  termination for cancel, timeout, disconnect, and stream write failure, so
  shell-spawned descendants do not hold stdout/stderr open after cancellation.

### Added — real streaming exec (m80-5vha)

- `ExecRequest::streaming` opts into real chunk-by-chunk stdout/stderr over
  the existing envelope wire. `ExecStdout` / `ExecStderr` frames are followed
  by one terminal `ExecExit`.
- `RunningSandbox::exec_streaming` exposes those chunks to library callers.
  Buffered `RunningSandbox::exec` is now built on top of the streaming path
  while preserving the existing 1 MiB per-stream cap.
- `m80 run` pipe mode consumes `exec_streaming`, so stdout/stderr reach the
  host before process exit and are not capped by the buffered response limit.
- Guestd cancellation covers both explicit `CancelRequest` and host
  disconnect. The read-side EOF path is required for silent commands, so a
  dropped streaming caller does not leave `sleep 600` running in the guest.

### Fixed — real-KVM perf-roadmap smoke blockers

- Minimal images now pre-create `/lower`, `/upper`, and `/merged` for the
  PID-1 overlay pivot. The base root is mounted read-only by the kernel, so
  guestd verifies these mountpoints instead of trying to create them at boot.
- `m80-guestd` no longer blocks an exec response while waiting for a possible
  cancel frame. The real vsock path polls for cancel readability before
  calling `fill_buf()`; in-memory tests pin that completed execs return even
  when no cancel bytes are readable.

### Performance — storage pivot measured (m80-f2zc.7)

Real-KVM bench on 2026-05-05 with a freshly rebuilt minimal image:

| cell | wallclock P50 | useful P50 | `phase_3_storage_prep` P50 | success |
|---|---:|---:|---:|---:|
| minimal / idle, N=30 | 1517 ms | 1207 ms | 215.9 ms | 30/30 |
| minimal / loaded, N=5 | 1644 ms | 1318 ms | 254.6 ms | 1/5 |

Compared with the pre-pivot minimal/idle baseline, `phase_3_storage_prep`
drops from 727.6 ms to 215.9 ms (-511.7 ms). A separate 16-VM concurrent
probe passed 16/16 in 1620 ms wallclock with only +4.4 MiB `/proc/meminfo`
Cached delta, consistent with a shared read-only base layer.

### Performance — stripped kernel measured (m80-ci9i.4)

The stripped kernel now boots through the non-PCI legacy-MMIO Firecracker path:
`CONFIG_VIRTIO_MMIO_CMDLINE_DEVICES=y`, `CONFIG_ACPI=n`, `CONFIG_PCI=n`, and
`pci=off` retained in the stripped cmdline. Earlier ACPI-only attempts panicked
before userspace because `/dev/vda` was never discovered. Ubuntu/systemd also
required the stripped keep-list to include cgroups, tmpfs ACL/xattr support,
file-handle syscalls, and the basic event primitives used during API
filesystem setup.

Real-KVM bench on 2026-05-05 showed the current stripped config is
boot-correct for the minimal image and modestly faster, but far short of the
expected 500-700 ms win:

| cell | wallclock P50 | useful P50 | `phase_12b_ready_accept` P50 | success |
|---|---:|---:|---:|---:|
| minimal / idle, N=30 | 1417 ms | 1167 ms | 866.0 ms | 30/30 |
| minimal / loaded, N=5 | none | 1242 ms from phases | 866.6 ms | 0/5 |
| ubuntu / idle, N=30 | 3818 ms | 2435 ms | 1006.8 ms | 30/30 |

Compared with the stock post-pivot minimal/idle run, wallclock P50 improved
by 100 ms and ready latency improved by about 40 ms. Compared with the matching
schema-3 stock Ubuntu run, wallclock P50 improved by 101 ms and ready latency
improved by about 101 ms. The expected 500-700 ms stripped-kernel win is not
validated by this config.

### Performance — persistent VM turn-to-turn measured (m80-qokt.2.6)

Real-KVM bench on 2026-05-05 with a single live Minimal VM:

| path | N | P50 | P95 | max |
|---|---:|---:|---:|---:|
| persistent `RunningSandbox::exec` | 30 | 20.589 ms | 20.716 ms | 21.183 ms |
| stock post-pivot cold launch | 30 | 1517 ms | 1518 ms | 1518 ms |

Sequential persistent exec saves about 1496 ms P50 after the first turn
relative to a fresh cold launch. The benchmark harness lives at
`crates/m80-firecracker/benches/persistent_turn_latency.rs`; raw samples are
recorded in `docs/behaviors/lifecycle/persistent-state-latency.json`.

### Performance — snapshot restore measured (m80-rrp.3.6)

`RunningSandbox::capture` and `Sandbox::launch_from_snapshot` now bind-mount
the caller's snapshot directory into the Firecracker jail at `/snapshot` before
issuing snapshot REST calls. This makes caller-managed host snapshot paths
visible to the jailed Firecracker process; without it, capture failed with
`Cannot perform open on the snapshot backing file`.

Real-KVM restore-ready bench on 2026-05-05:

| host load | N | restore P50 | restore P95 | max |
|---|---:|---:|---:|---:|
| idle | 50 | 274.204 ms | 280.132 ms | 289.409 ms |
| loaded (`stress-ng --cpu $(nproc)`) | 50 | 444.972 ms | 588.221 ms | 1834.719 ms |

Against the stock post-pivot cold-launch checkpoint (`minimal/idle` P50
1517 ms), idle snapshot restore is about 5.5x faster and saves about 1243 ms
per allocation. Harness:
`crates/m80-firecracker/benches/snapshot_restore_latency.rs`; raw samples:
`docs/behaviors/snapshot/restore-latency.json`.

### Performance — warm-pool allocation measured (m80-rrp.6)

`WarmPool` now pre-restores guestd-ready slots from a captured snapshot and
hands out `WarmLease` values without a cold boot or synchronous restore on the
allocation path. Empty pools return `FcError::PoolEmpty`; allocation never
hides a cold-boot fallback. Leases default to discard-and-refill unless complete
blank-VM reset evidence is supplied.

Real-KVM allocation bench on 2026-05-05 with N=50 and one ready slot:

| host load | allocation P50 | allocation P95 | max | refill P95 |
|---|---:|---:|---:|---:|
| idle | 20.957 ms | 21.053 ms | 21.070 ms | 1261.443 ms |
| loaded (`stress-ng --cpu $(nproc)`) | 21.005 ms | 623.409 ms | 1334.006 ms | 1535.415 ms |

The loaded host still exposes a Firecracker restored-vsock local-init tail.
`RunningSandbox::exec` now retries only the open+send phase for a fixed 2.5 s
budget; receive-side failures still surface immediately because the guest may
already have run the request. Raw samples:
`docs/behaviors/lifecycle/warm-pool-allocation-latency-{idle,loaded}.json`.

### Added — perf-roadmap structural landings (Waves 0-4)

Four-step path to sub-200 ms warm-pool launch landed at the structural
level (designs, APIs, tests compile, workspace green). Real-KVM bench
numbers for the original BENCH leaves are captured in the sections above.

**Storage pivot (m80-f2zc)** — RO base + per-VM sparse overlay + in-guest
overlayfs + `pivot_root`:
- `m80-storage`: `Rootfs::prepare(base, overlay_dest, overlay_size_bytes)`
  replaces the old per-VM full-file `Rootfs::clone`. Sparse alloc + `mkfs.ext4 -F`.
  Errors: `OverlayCreateFailed`, `MkfsFailed`.
- `m80-firecracker`: `phase_11_rest_puts` now PUTs vda (RO base) → vdb
  (RW overlay) → vdc (RW workspace, when present). `is_root_device: true`
  on vda. `SandboxConfig::overlay_size_bytes` (default 512 MiB).
- `m80-guestd` (PID-1, Minimal): mounts overlayfs (lower=/vda, upper=/vdb/upper,
  work=/vdb/work) and `pivot_root`s onto it. `pivot_rootfs` lifted verbatim
  from kata-containers `mount.rs:523-559` with SPDX-Apache-2.0 attribution.
  Failure = panic (kernel panic via `panic=-1`); stderr instrumented per
  CLAUDE.md "diagnostics before hypotheses".
- Design: `docs/design/storage-overlay.md`.

**Stripped kernel (m80-ci9i)** — purpose-built minimal kernel:
- `m80-image-manifest`: schema bumped 2→3 with `KernelKind { Stock, Stripped }`
  (`#[serde(default)]` so existing v2 manifests deserialize unchanged).
- `m80-image-build`: `kernel-builder/{Dockerfile, m80-stripped.config, build.sh}`
  produces `kernels/vmlinux-m80-<config-sha>.bin`. Linux 6.1.134 LTS pinned.
  CONFIG keep-list mandates `CONFIG_OVERLAY_FS=y` + `CONFIG_OVERLAY_FS_XINO_AUTO=y`.
  `--kernel stock|stripped` flag on the image-build CLI.
- `m80-firecracker`: `boot_args_for(image_kind, kernel_kind)` two-axis
  dispatch. Stripped baseline: `console=ttyS0 reboot=k panic=-1 pci=off
  quiet loglevel=0 8250.nr_uarts=1` (uarts=1, not 0 — diagnostic visibility
  worth ~50 ms).
- Design: `docs/design/stripped-kernel.md`.

**Snapshot/restore (m80-rrp.3)** — Firecracker snapshot REST + warm-pool plumbing:
- `m80-firecracker-client`: `put_snapshot_create`, `put_snapshot_load`,
  `patch_vm_state`. Wire verified against Firecracker `swagger/firecracker.yaml`.
- `m80-snapshot`: `capture(req)` / `restore(req)` primitives. Restore mandates
  `unlink(vsock.sock)` before `PUT /snapshot/load` (empirically: stale UDS
  causes EADDRINUSE).
- `m80-firecracker`: `Sandbox::launch_from_snapshot` and `RunningSandbox::capture`.
  Restore-path readiness uses `phase_restore_probe_exec_channel` (probes
  `CONNECT 9001` directly) — vsock connections do NOT survive snapshot
  (TRANSPORT_RESET) but guest LISTEN sockets do. Cold-boot readiness
  (m80-7tpy inverted readiness) unchanged.
- `m80-cli`: `m80 launch --from-snapshot <DIR>` and `m80 snapshot capture
  <vm-id> <DIR>` (capture is a v0.1 stub).
- Empirical: `docs/exploration/firecracker-vsock-snapshot.md`. Design: `docs/design/snapshot-restore.md`.

**Persistent-VM mode (m80-qokt.2)** — multi-exec on one VM:
- `RunningSandbox::exec(&mut self, …)` formally supports sequential calls;
  filesystem, `/tmp`, env-via-shell-history persist between calls.
- `m80-proto::types`: `CancelRequest`, `CancelAck`, `CancelStatus` envelopes.
  Guestd dispatches Cancel mid-exec (try_wait + poll loop, SIGKILL on match).
  Shared with `m80-5vha` (streaming exec); first-to-land owned the type.
- `SandboxConfig::idle_timeout: Option<Duration>` (default 5 min). Background
  watcher resets on `exec`; on expiry, next `exec` returns `FcError::IdleTimedOut`.
- Behavior docs: `docs/behaviors/lifecycle/{persistent-state,exec-cancellation,idle-timeout}.md`.
  Design: `docs/design/persistent-vm.md`.

**Process notes**: dispatched as 5 waves of parallel sonnets per
`docs/planning/parallel-execution-strategy.md` and the extended risk
register in `docs/planning/perf-roadmap-extended.md`; ~9 days wall-clock vs
22-27 sequential person-days estimate. Workspace stayed green at every wave
gate. BENCH leaves `m80-f2zc.7`, `m80-rrp.3.6`, `m80-qokt.2.6`, and
`m80-ci9i.4` now have real-KVM measurements.

### Added — bench snapshot + diff tooling (m80-vf7o)

`scripts/bench-summary.py` extracts the inline Python from `bench-cold-launch.sh` and adds `summarize`, `compute`, and `diff` subcommands; `bench-cold-launch.sh` auto-saves a per-run JSON snapshot to `crates/m80-firecracker/benches/snapshots/` (symlinked as `latest.json`) so perf iterations can be compared with `python3 scripts/bench-summary.py diff baseline.json latest.json [--fail-on-regress N]`.

### Added — test helper: `RunDirDumpGuard` (m80-83y9)

`tests/common::RunDirDumpGuard` is a drop-guard for integration tests: on test failure (i.e. when the thread is panicking) it dumps the last 100 lines of `<run_dir>/console.log` and the full `diagnostics.jsonl` to stderr, including the run-dir path for offline re-inspection. Passing tests produce no output. Opt-in via a named local binding.

### Added — wire-level debug dump (`M80_DEBUG_WIRE`)

Set `M80_DEBUG_WIRE=vsock`, `M80_DEBUG_WIRE=fcrest`, or `M80_DEBUG_WIRE=all` to get `tracing::trace!` output of every vsock frame and Firecracker REST call (hex+ASCII preview, capped at 1024 bytes). Zero overhead when unset.

### Reliability — ubuntu/idle 40 % flake eliminated

The ubuntu/idle launch-failure rate was ~40-50 % under a tight loop;
post-fix it is **20/20 = 100 %** at N=20. Two compounding causes were
addressed in commit `ad00c02`:

1. **Inverted-readiness vsock signal** (m80-7tpy). The previous
   polled-CONNECT/OK probe (10 ms cadence into Firecracker's vsock
   muxer) provoked a `vsock: error adding local-init connection
   (WouldBlock)` race in the muxer's accept loop, surfaced as
   `Broken pipe` on the host. Replaced with: m80-guestd connects out
   to the host on `READY_PORT_DEFAULT = 52525` with a single
   `PROTOCOL_VERSION` byte; the host pre-creates a `UnixListener`
   at `<vsock_uds>_<READY_PORT>` and `accept()`s. Event-driven, no
   polling, no muxer race.

2. **Service-unit ordering cycle** (the actual majority cause).
   `m80-guestd.service` was `WantedBy=multi-user.target` +
   `After=multi-user.target`, which dragged in
   `network-wait-online`'s bimodal timeout (~1.5 s on success vs
   full 90 s on hang) — m80-guestd started at T=91 s on the bad
   path, well past the host's `READY_TIMEOUT`. Fixed by
   `DefaultDependencies=no`, no `After=` at all (initial attempt
   added `After=workspace.mount` but that formed an ordering cycle
   with `local-fs.target`; systemd's non-deterministic cycle-break
   sometimes deleted the m80-guestd job entirely), `WantedBy=
   basic.target`. Image-build symlinks `m80-guestd.service` under
   `basic.target.wants/`; `workspace.mount` stays under
   `multi-user.target.wants/`.

Polling-related code in `m80-vsock` (`watch_ready_marker`, the
five-argument `Channel::open`, `READY_POLL_INTERVAL`) is removed in
the same commit — fully replaced by the inverted-readiness path.

### Performance — cold-launch headline

After m80-bgas.1, m80-6a0q, and the vsock graceful-stop migration:

| kind / load     | total wallclock (P50) | useful (P50, ex-stop) | success rate |
|-----------------|----------------------:|----------------------:|-------------:|
| ubuntu / idle   |              8 922 ms |              5 692 ms |       18/30  |
| minimal / idle  |              3 018 ms |              1 696 ms |       30/30  |

**Minimal/idle is 3.4× faster than ubuntu/idle** for `launch + exec`,
dominated by a smaller rootfs clone (5.5×) and skipping systemd boot
(1.7×). See `docs/perf/cold-launch.md` for per-phase attribution + the
re-run procedure.

`stress-ng --cpu $(nproc)` loaded cells fail 100% on both kinds today
— captured as a known issue, not a release blocker.

### Added — vsock graceful-stop (path c)

Replaces `SendCtrlAltDel`-and-wait-30-s with a `ShutdownRequest` /
`ShutdownResponse` envelope on the existing vsock channel.

- **`m80-proto`** gains `ShutdownRequest`, `ShutdownResponse`,
  `ShutdownAction { Exit | Poweroff }` payload types and the matching
  `PAYLOAD_KIND_*` constants.
- **`m80-guestd`** dispatches incoming envelopes by `kind`. On
  `shutdown_request`: sync filesystems, ack with the chosen action,
  flush, then exit (PID 1 → kernel panic → reboot, panic=1) or
  `/sbin/poweroff -f` (systemd-managed under ubuntu).
- **`m80-firecracker::lifecycle::bounded_stop`** opens a fresh vsock
  channel, sends `ShutdownRequest`, reads `ShutdownResponse`, then
  polls the firecracker pid for up to `GRACEFUL_STOP_TIMEOUT` (2 s,
  was 30 s) before falling back to SIGKILL. Drops the
  `m80_firecracker_client::Client` parameter from `bounded_stop` since
  `SendCtrlAltDel` is gone.
- `stop_bounded` phase: 30 022 ms → 2 006 ms ubuntu, 1 061 ms minimal.

### Added — per-phase timing instrumentation

`M80_PHASE_TRACE=1` causes `Sandbox::launch`, `RunningSandbox::exec`,
and `RunningSandbox::stop` to emit one `M80_PHASE name=… vm_id=…
elapsed_us=…` line on stderr per phase boundary. No-op when unset
(production stderr stays quiet).

`scripts/bench-cold-launch.sh`:
- captures stderr per launch, parses `M80_PHASE` lines into
  `cold-launch-phases.csv` (long format)
- hard-fails if `stress-ng` is missing (was silently skipping the
  loaded cells)
- separates success vs failure counts; computes percentiles from
  successes only
- prints a `useful_ms` rollup that subtracts `stop_bounded` from the
  per-launch wallclock

### Added — minimal image kind (m80-6a0q)

- **Minimal image kind** (m80-6a0q) — alongside the existing
  Ubuntu-with-systemd image, m80 now builds a busybox + statically-
  linked m80-guestd rootfs that runs as PID 1, no systemd. Smaller
  rootfs (256 MiB default vs 1 GiB), faster cold boot. See
  `crates/m80-image-build/README.md` for the kind-comparison table.
  - **`m80-image-manifest::ImageKind { Ubuntu, Minimal }`** discriminator
    on `Manifest`, with kind-conditional `Option<>` fields for
    systemd-related and source-rootfs artifacts.
  - **`m80-image-build`** gains a `[rootfs] kind = "minimal"` config
    that branches the pipeline (mkfs from scratch, copy host
    `/bin/busybox` + applets, install static guestd at `/m80-guestd`,
    symlink `/init`).
  - **`m80-guestd`** detects PID-1 mode at startup and mounts `/proc`,
    `/sys`, `/dev`, plus `/dev/vdb → /workspace` if attached;
    poll-reaps orphan children between requests.
  - **`m80-firecracker`** dispatches kernel boot args on
    `manifest.image_kind`: Minimal appends `init=/m80-guestd` so the
    kernel calls our daemon directly.
  - **`scripts/smoke.sh`** gains `M80_IMAGE_KIND=minimal` (auto-installs
    musl rustup target, builds static guestd, requires
    `/bin/busybox` from `busybox-static`).
  - **`scripts/bench-cold-launch.sh`** + `docs/perf/cold-launch.md`
    capture methodology for ubuntu-vs-minimal P50/P95 measurements.

### Changed — manifest schema v1 → v2 (m80-6a0q.4)

- **Manifest schema bumped 1 → 2** (m80-6a0q.4 prerequisite). No
  conversion code; existing v1 images must be rebuilt. New required
  field `image_kind`. Five fields become `Option<>` with a kind-
  conditional invariant enforced on read/write/verify.

### Performance — ready-probe cadence tightened (m80-bgas.1)

- **`phase_12b_ready_probe` cadence tightened** (m80-bgas.1) —
  `READY_POLL_INTERVAL` 500 ms → 10 ms; `READY_TIMEOUT` 30 s → 60 s.
  Smolvm proves a 10 ms cadence is safe under load (their
  `FAST_POLL_INTERVAL`); each `Channel::open_uds_only` attempt returns
  in microseconds when the guest isn't ready and ~10 ms when it is, so
  10 ms polling adds at most one wasted RTT per probe. Cuts median
  ready-probe latency above the guest-boot floor from ~250 ms to
  ~5 ms.

### Changed — smoke retry loop dropped (m80-bgas.2)

- **`scripts/smoke.sh` retry loop dropped** (m80-bgas.2) — the script
  previously retried `m80 launch` up to 3× to mask a v0.1 vsock-probe
  flake under host load. With the tightened cadence + extended timeout
  above, one attempt is sufficient. Per-launch wallclock budget bumped
  from 60 s to 90 s to leave headroom for the new 60 s ready-probe
  ceiling.

## [v0.1.0-smoke-passing] — 2026-05-04

End-to-end microVM launch works on KVM. `m80 launch -- /bin/echo hello`
boots a firecracker VM, runs the command, and exits cleanly.

### Added

- **`m80` CLI** — `preflight`, `launch`, `exec`, `cleanup`, `inspect`,
  `list`, `stop`, `config show`, `version`. Stable per-class exit
  codes (`EXIT_PREFLIGHT=2`, `EXIT_ADMISSION=3`, `EXIT_MANIFEST=4`,
  `EXIT_INVALID_STATE=5`, `EXIT_CONFIG=6`, `EXIT_NOT_IMPLEMENTED=7`),
  `--json` envelope mode for scripted callers.
- **`m80-firecracker`** — 12-phase preboot pipeline composing every
  foundation crate. Type-state lifecycle (Sandbox → RunningSandbox →
  StoppedSandbox), admission semaphore with permit return on Drop,
  `Backend::recover_stale_run_root()` that handles dead-jail / orphan-
  firecracker / no-jail cases (kills orphans, unmounts via mountinfo,
  removes cgroup leaf, rms run-dir).
- **`m80-image-build`** — pulls kernel + rootfs from firecracker-ci S3,
  loop-mounts and installs `m80-guestd` + systemd units, emits provenance
  manifest. Subcommands: `run`, `verify`, `clean`.
- **`m80-preflight`** — 10-check capability gate (KVM, kernel modules,
  privilege caps, firecracker+jailer binaries, kernel image, rootfs +
  manifest, run-root with `nodev` rejection, storage helpers, OS gate).
- **`m80-jailer`** — bind-plan + materialize + jailer launch + stale
  recovery. Source split: `types.rs`, `plan.rs`, `materialized.rs`,
  `recover.rs`, `error.rs`.
- **`m80-firecracker-client`** — sync HTTP-over-UDS REST client with
  typed per-endpoint errors.
- **`m80-cgroup`** — per-VM cgroup v2 subtree under `m80-firecracker/`.
- **`m80-storage`** — rootfs clone + scratch ext4 create/hydrate +
  post-stop change extraction.
- **`m80-vsock`** — host-side bridge with `Channel::open_uds_only` for
  callers that establish guest readiness without a console-log file.
- **`m80-proto`** — wire schemas (`Envelope<T>`, `ExecRequest`,
  `ExecResponse`, `ExecStatus`, `ExecTiming`, `HandshakeMessage`),
  length-prefixed framing.
- **`m80-image-manifest`** — guest-image provenance schema with
  `Manifest::verify` (recomputes sha256 of every artifact).
- **`m80-net-mode`** — `NetworkPolicy` (caller intent) → `VmNetworkMode`
  (resolved) split. `OutboundNat` deferred to v0.2.
- **`m80-guestd`** — in-VM daemon binary. Listens on vsock port 9001,
  serves one `Envelope<ExecRequest>` per connection.
- **`scripts/smoke.sh`** — runnable end-to-end smoke test capturing the
  env-var dance.
- **CI** — minimal GitHub Actions workflow: fmt-check, build, test,
  clippy on push + PR to main.

### Smoke-test fixes

Discovered while running the first end-to-end launch on a real KVM host:

- **Jailer chroot path** — `m80-jailer` was placing bind mounts in
  `<run_dir>/jail/`, but the official jailer binary creates its chroot
  at `<run_dir>/<exec basename>/<id>/root/`. New `chroot_path()` helper
  derives jailer's actual layout.
- **`--api-sock` placement** — was passed as a jailer arg; firecracker
  rejected. Moved past the `--` separator so jailer forwards it.
- **No `--daemonize`** — without it, jailer `exec()`s into firecracker
  so the spawned `Child` handle's pid IS firecracker. The previous
  `child.wait()` blocked forever waiting for the VM to exit. Switched
  to `mem::forget(child)` + Drop-side kill+reap.
- **RW bind chown** — bind-mounted `rootfs.ext4` was owned by host root;
  jailed firecracker (uid 3000) couldn't open it for writing. RW binds
  now `chown` the source to the jail uid:gid.
- **`/tmp` is `nodev`** — the kernel forbids opening device nodes on a
  `nodev` filesystem regardless of file permissions, so `/dev/kvm`
  inside a chroot under `/tmp` failed with EACCES on `InstanceStart`.
  `m80-preflight` now rejects `nodev` run-roots up-front. Default
  switched to `/var/lib/m80-run`.
- **Vsock `open_uds_only`** — `Channel::open` watches a console-log
  file for the ready marker first; that watch was returning
  `NotReady` and short-circuiting the UDS connect. Added
  `Channel::open_uds_only` for callers that establish readiness via
  another channel (the orchestrator's polling loop on the UDS itself).
- **Image-build alignment** — the `firecracker-ci` S3 bucket layout
  changed; updated kernel + rootfs paths to match what's actually
  shipped today (`vmlinux-5.10.245`, `ubuntu-24.04.squashfs`).
- **Mkfs target sizing** — `mkfs.ext4 -d srcdir target` requires the
  target file pre-allocated; now `truncate`d before mkfs.
- **Manifest daemon-binary path** — was the in-VM destination, but
  `Manifest::verify` walks every path on the host filesystem.
  `m80-image-build` now writes a host audit copy and references it.
- **Stale-recovery teardown** — `recover_stale_run_root` previously
  failed with EBUSY on bind mounts left by SIGKILL'd VMs. New helper
  reads `/proc/self/mountinfo` and `umount2(MNT_DETACH)`s every
  mountpoint at-or-below the stale run-dir before `rm -rf`. Also
  cleans the cgroup leaf, kills any orphaned firecracker process.

### Trimmed

A 16-crate / one workspace-pass `/trim-the-fat` sweep removed ~2,150
lines of cosmetic ceremony, dead surface, and stale documentation:

- Forbidden `#[non_exhaustive]` on internal `ExecStatus` (CLAUDE.md).
- Dead `ExecRequest::workspace_dir` wire field (no producer, no consumer).
- Deferred-v0.2 `put_network_interface` / `NetworkInterfaceConfig` (will
  return when `OutboundNat` lands).
- Many narrative comments restating visible code; many README sections
  duplicating rustdoc; many ASCII section dividers.
- Stale comments left over from the smoke-test fixes (jailer reaping,
  chroot path, `--daemonize` semantics).
- Lossy `map_err(|_|)` sites in config parsing + preflight that hid
  underlying error context.
- DRY: `SnapshotManifest`/`RestoreMetadata` write/read into shared
  helpers; `m80-storage` mount/umount triplicate into one helper;
  `Channel::close`/`Drop` into one `teardown`.

### Project state at this tag

- 16 crates, ~7,000 LOC src.
- All workspace tests pass (modulo `m80-guestd::exec_with_timeout_returns_timed_out`
  timing flake under heavy parallel load — passes solo, mitigated in CI
  with `--test-threads=2`).
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `cargo fmt --all -- --check` clean.
- End-to-end verified by `scripts/smoke.sh`.

### Deferred to v0.2

- `m80-net-outbound` — bridge/tap/iptables/DNS for `OutboundNat` mode.
  v0.1 rejects `OutboundNat` in phase 6 with a clear error.
- `m80-snapshot` capture/restore execution — schemas + persistence
  paths active; `capture()`/`restore()` return `Deferred`.
- `m80-observability` Probe + `render_prometheus` execution — types
  pinned, execution returns `Deferred`. `Diagnostics::disabled()` works.
- Warm pool / persistent-state lifecycle (epic m80-rrp).
- `m80-adapter` agent-semantics layer on top of m80-firecracker (epic
  m80-qokt.1).
- `m80 exec` out-of-process IPC (the CLI subcommand returns
  `EXIT_NOT_IMPLEMENTED` in v0.1; library callers use
  `RunningSandbox::exec()` directly).

## [pre-v0.1] — 2026-04-29 to 2026-05-03

Bead-capture phase + initial implementation waves. See `git log` for
detail; relevant commits: `c738ba7` (initial bootstrap), `8333052`
(wave-1b: 7 foundation crates), `4403ea9` (wave-2-3: cgroup,
firecracker orchestrator, cli).
