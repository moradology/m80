# Perf measurement playbook

This playbook is the index for the measurement-track work under
`m80-jp6ik`. Each experiment below states when to run it, the exact harness
entry point, where the result lands, and how later gated beads cite the
measurement.

Measurement-shaped beads use the scaffolded-vs-verified discipline from
ADR 0002. A harness, parser, or mock run is scaffolded. A bead is verified only
when the committed artifact came from the real substrate named by that
experiment: KVM host, `/dev/kvm`, `sudo`, Firecracker/jailer binaries, and real
m80 images.

## Shared Rules

- Run before/after comparisons on the same host in the same session.
- Use `N >= 20` per cell for directional claims and `N >= 50` for tail-latency
  or close-gate claims.
- Commit the raw artifact and the interpretation doc. Do not close from console
  output alone.
- Record host context in the interpretation doc: kernel, Firecracker version,
  image path or artifact hash, CPU count, CPU driver/governor when relevant, and
  whether page cache was dropped.
- If a run uses temporary instrumentation, keep the instrumentation commit,
  artifact commit, and cleanup commit easy to audit.
- Do not rely on implementation-specific `grep` behavior in reproduction
  instructions. Prefer structured outputs, `rg`, Python parsing, or simple POSIX
  shell constructs.
- Close a measured bead with `verified: <artifact-path> @ <commit-sha>`.

## Q420K Close-Artifact Guard

The remaining `m80-q420k` measurement leaves also have a machine-readable field
guard:

```sh
python3 scripts/verify-q420k-artifacts.py
```

Before producing any close-quality real-KVM artifact, run
`./scripts/q420k-quiet-host-inventory.sh`. It exits 0 only on a quiet host and
prints owner inventory without terminating anything when unrelated Firecracker
processes are present.

Use `--only pmem-density` for the Phase C Shared density markdown artifact and
its executable `scripts/smoke-pmem-shared.sh` close script, and use
`--only snapshot-template` for the Phase D restore-latency JSON plus its
`docs/perf/snapshot-template-restore.md` reproduction and smoke-evidence doc
before the other artifacts exist. Repeat `--only` for a subset, for example
`--only composed-restore --only composed-memory --only composed-residue`.
Use `--only composed-doc` for the Phase F receipt doc plus the three composed
JSON artifacts it interprets. Use
`--only ext4-overlay` or `--only dax-memory-pressure` only for Phase G
followups; they are intentionally not part of the default A-F parent close
guard.

Subset checks are leaf/preflight tools, not final-close evidence. Do not
combine `--only` with `--require-parent-phases-closed` or
`--require-super-epic-closed`; the verifier rejects that combination so a
narrow green check cannot be mistaken for the full A-F close gate. The parent
close flag also requires both `--require-committed` and
`--require-closed-beads`; the super-epic close flag requires the parent close
flag.

This guard rejects missing artifacts, artifacts that do not name the real-KVM
preflight substrate, noisy-host override artifacts, weak sample counts,
threshold misses, teardown residue, Shared payload layout drift found by
`m80-q420k.8.12`, a missing or non-executable quiet-host inventory helper,
a missing or non-executable Shared density smoke script,
snapshot-template docs without the matching bench stderr paste, snapshot
artifact commands that do not pin `M80_SNAPSHOT_TEMPLATE_ALLOW_OTHER_VMS=0`,
`target_ready=1`, vCPU/memory sizing, run-root inputs, and sample arrays that
back the exact `n_per_run=20`, `runs=3`, `samples_total=60` matrix and its
`p99`, missing or duplicated snapshot-template `(run, cycle)` sample coverage,
missing per-run detail, and a composed receipt doc that still carries the
diagnostic banner.
Measurement artifacts that carry source-tree cleanliness fields must record a
clean worktree except for the artifact paths themselves. Real-KVM close
artifacts also record empty pre-run and post-run Firecracker process lists,
proving both a quiet starting host and no Firecracker leak after teardown.
They also record preflight artifact identity: resolved Firecracker,
jailer, seccomp filter, helper, kernel, and rootfs paths plus manifest
kernel/rootfs sha256s. Each close artifact records the full measured
`git_commit` alongside the clean-worktree flag. The snapshot-template receipt
doc is cross-checked against its JSON artifact for measured git commit,
preflight identity, command-block env assignments, and smoke p99; prose outside
the command block does not satisfy command requirements. Add
`--require-committed` after the artifacts are committed; that mode rejects
artifacts absent from `HEAD` or with staged/unstaged changes. For final parent
close, add `--require-closed-beads` and
`--require-parent-phases-closed` so the guard also checks the verified close
reasons, verifies that each cited commit exists in `HEAD` history and contains
the named artifact, verifies that the cited commit descends from the
artifact's measured `git_commit`, verifies that the artifact has not changed
since that commit, checks Phase 0 / A-F parent statuses, and checks that the
Phase C, Phase D, and Phase F parent close reasons reuse the matching child
evidence commits. It is not proof of substrate by itself. The final guard also
checks q420k blocker rows and dependency cycles so stale tracker edges cannot
be ignored. The close still needs the real-KVM run and
`verified: <artifact-path> @ <commit-sha>` reason.
After `m80-q420k` itself is closed, add `--require-super-epic-closed` to check
that the super-epic close reason reuses the same
`verified: docs/perf/composed-e2e.md @ <commit-sha>` evidence commit as
`m80-q420k.6`.

## Q420K Shared Pmem Density

Gates: `m80-q420k.3.8` and therefore parent Phase C.

Purpose: prove that `PmemSharing::Shared` reuses one canonical image-store
inode across multiple VMs and produces bounded host-memory growth after the
guests actually fault the Shared payload.

Invocation:

```sh
M80_PMEM_SHARED_ALLOW_OTHER_VMS=0 \
M80_PMEM_SHARED_VM_COUNT=4 \
M80_PMEM_SHARED_CYCLES=10 \
M80_PMEM_SHARED_PAYLOAD_MIB=128 \
M80_PMEM_SHARED_PER_VM_OVERHEAD_KIB=131072 \
M80_PMEM_SHARED_DENSITY_ARTIFACT=docs/perf/pmem-shared-density.md \
M80_RUN_ROOT=/var/lib/m80-psd \
M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker \
M80_JAILER_BIN=/opt/firecracker/bin/jailer \
M80_FIRECRACKER_SECCOMP_FILTER=/opt/firecracker/bin/firecracker-seccomp-filter.bin \
M80_JAILER_HARDEN_BIN=/opt/m80/bin/m80-jailer-harden \
M80_NET_HELPER_BIN=/opt/m80/bin/m80-net-helper \
M80_KERNEL_IMAGE=<real-stripped-kernel.bin> \
M80_KERNEL_KIND=stripped \
M80_ROOTFS_IMAGE=<real-rootfs.ext4> \
M80_FIRECRACKER_VERSION=v1.15.1 \
M80_JAIL_UID="$(id -u)" \
M80_JAIL_GID="$(id -g)" \
./scripts/smoke-pmem-shared.sh
```

Artifacts:

- `docs/perf/pmem-shared-density.md`

Required interpretation: quiet-host substrate, host kernel >= 6.5, real
`/dev/kvm` rw, sudo/root uid evidence, Firecracker version output matching
preflight, page-cache drop before each cycle, `vm_count >= 4`, `cycles >= 10`,
active-use marker count returns to zero after teardown, canonical Shared
artifact remains present, stale-marker sweep removes zero markers, post-run
Firecracker process list is empty, preflight artifact identity is present, max
observed host-memory delta stays within the computed bound, the bound equals
`image KiB + per-VM overhead KiB * vm_count`, the `Samples` table has one row
per cycle and recomputes the max delta, the measured git commit is recorded,
the Shared image digest is a lowercase sha256 with an absolute image path
containing that digest, the reproduction command matches the artifact's
preflight Firecracker, jailer, helper, kernel, rootfs, version, and kernel-kind
values, and the payload erofs dump matches the `.8.12` file-level DAX
requirement (`Layout: 0` with equal logical and on-disk size). The verifier
reads the teardown fields
directly from the artifact:
`max active-use markers observed`, `final active-use markers`,
`stale markers swept after teardown`, and
`canonical Shared artifact present after teardown`. The artifact must also
carry the `TrustDomainAck`, same-trust-domain, DAX cache-timing side-channel,
and read-only Shared jail-binding statements used by the parent close prose.
Before closing, after
committing the artifact, run:

```sh
python3 scripts/verify-q420k-artifacts.py --only pmem-density --require-committed
```

That selector also checks `scripts/smoke-pmem-shared.sh`; the script must be
executable, retain the quiet-host fail-closed guard in uncommented code, pass
the exact helper/kernel/rootfs/run-root paths through sudo, emit the artifact's
reproduction command, and be committed without mode or content drift when
`--require-committed` is present. The density artifact's reproduction command
is parsed as exact env assignments; prefix matches such as
`M80_PMEM_SHARED_VM_COUNT=40` do not satisfy an expected
`M80_PMEM_SHARED_VM_COUNT=4`.

## Q420K Ext4 Overlay-Template Clone

Gates: `m80-q420k.8.16`, a Phase G followup. This does not block the A-F
super-epic close, but it is the evidence gate for reconsidering dm-snapshot on
ext4 run roots.

Purpose: isolate the explicit byte-copy fallback for a run-root-local empty
overlay template on ext4 and compare the result to the dm-snapshot reconsider
threshold.

Invocation:

```sh
M80_RUN_EXT4_OVERLAY_TEMPLATE_CLONE=1 \
M80_EXT4_OVERLAY_TEMPLATE_SAMPLES=30 \
M80_EXT4_OVERLAY_TEMPLATE_RUN_ROOT=/var/tmp/m80-ext4-overlay-template-clone \
M80_EXT4_OVERLAY_TEMPLATE_ARTIFACT=docs/perf/ext4-overlay-template-clone.md \
cargo test -p m80-storage --test ext4_overlay_template_clone -- --ignored --nocapture
```

Artifact:

- `docs/perf/ext4-overlay-template-clone.md`

Required interpretation: run root filesystem is ext4, `samples >= 30`,
`phase_3b_rootfs_prepare` P50/P95/P99 are recorded, leaked dm devices = 0,
leaked mounts = 0, and the doc states whether dm-snapshot was prototyped. Do
not implement a dm-snapshot prototype unless the byte-copy result exceeds 80 ms
P50 or 100 ms P95, or an operator supplies residue/pressure evidence. Before
closing, after committing the artifact, run:

```sh
python3 scripts/verify-q420k-artifacts.py --only ext4-overlay --require-committed
```

## Q420K DAX Memory Pressure

Gates: `m80-q420k.8.9`, a Phase G followup. This does not block the A-F
super-epic close, but it is the evidence gate for deciding whether the existing
same-trust-domain Shared pmem side-channel language is enough under memory
pressure.

Purpose: measure how host or sibling-VM memory pressure changes read/refault
latency for the `.8.12`-valid Shared erofs payload layout and record whether
that timing signal is acceptable under the explicit trust-domain assumption.

Invocation:

```sh
M80_RUN_PMEM_DAX_MEMORY_PRESSURE=1 \
M80_PMEM_DAX_MEMORY_PRESSURE_COMMAND='stress-ng --vm 1 --vm-bytes 50% --timeout 30s' \
M80_PMEM_DAX_MEMORY_PRESSURE_VM_COUNT=2 \
M80_PMEM_DAX_MEMORY_PRESSURE_SAMPLES=5 \
M80_PMEM_DAX_MEMORY_PRESSURE_PAYLOAD_MIB=32 \
M80_PMEM_DAX_MEMORY_PRESSURE_ARTIFACT=docs/perf/pmem-dax-memory-pressure.md \
cargo test -p m80-firecracker --test pmem_dax_memory_pressure_real_kvm -- --ignored --nocapture
```

Artifact:

- `docs/perf/pmem-dax-memory-pressure.md`

Required interpretation: real-KVM substrate, actual Shared pmem guests, full
measured commit, quiet-host Firecracker substrate JSON with empty pre-run and
post-run Firecracker process lists, `VM count >= 2`, `samples per guest >= 3`,
payload size, lowercase sha256 image digest with an absolute image path
containing that digest, uncompressed non-inlined Shared payload layout
(`Layout: 0` with equal logical and on-disk size), baseline read latency
P50/P95/P99, post-pressure read/refault latency P50/P95/P99, cross-guest signal
delta recomputed from the P50 values, host memory and page-cache deltas
before/during/after pressure, leaked Shared markers = 0, leaked
Firecracker/jailer processes = 0, leaked mounts = 0, and a decision output that
mentions the same-trust-domain interpretation and either updates residual-risk
docs or files mitigation beads. Before closing, after committing the artifact,
run:

```sh
python3 scripts/verify-q420k-artifacts.py --only dax-memory-pressure --require-committed
```

## E1. Density Extended

Gates: `m80-jp6ik.13`, `m80-jp6ik.20`.

Purpose: find the first concurrency wall above the currently characterized
C=8 envelope, then attribute whether the wall is no-egress host setup, outbound
network setup, cgroup setup, or a non-m80 host limit.

Invocation:

```sh
N=20 WARMUP=2 KIND=minimal KERNEL_KIND=stripped \
  ./scripts/bench-density-extended.sh
```

The wrapper runs no-egress C=1,2,4,8,16,32,48,64 and outbound
C=1,2,4,8,16,32 through `bench-cold-launch.sh` with `CONCURRENT` and `EGRESS`.
C=64 is an oversubscription probe on the 48-CPU bench host, not a production
target.

Artifacts:

- `crates/m80-firecracker/benches/concurrent.csv`
- `crates/m80-firecracker/benches/snapshots/density-extended.json`
- `docs/perf/density-extended.md`

Required interpretation: table of concurrency by network mode with
wall-time-to-all-ready P50/P95/P99, per-VM P99, success rate, and a named
first-wall point.

## E2. Cold-Cold Restore Baseline

Gates: `m80-jp6ik.34`.

Purpose: establish restore-path cold-cache tax before adding page-cache priming
for snapshot files.

Invocation:

```sh
N=50 ./scripts/bench-restore-cold.sh --cold-isolation
```

If the restore harness lands inside `scripts/bench-cold-launch.sh`, use the
equivalent restore-mode flag and record the exact command in
`docs/perf/restore-latency.md`.

Artifacts:

- `crates/m80-firecracker/benches/snapshots/cold-restore-N50.json`
- `docs/perf/restore-latency.md`

Required interpretation: cold-cache and warm-cache restore P50/P95/P99, plus
restore-phase decomposition that identifies `mem.snap`, `vm.snap`, and any
other file read tax separately.

## E3. Mem-Size Sweep

Gates: `m80-jp6ik.29`.

Purpose: decide whether the default guest RAM size can move from 1024 MiB to
512 MiB without trading away the workload envelope m80 needs.

Invocation:

```sh
SWEEP=mem_mib SWEEP_VALUES=256,512,1024,2048 KIND=minimal SKIP_LOADED=1 N=30 \
  ./scripts/bench-cold-launch.sh
```

Artifacts:

- `crates/m80-firecracker/benches/snapshots/mem-size-sweep.json`
- `crates/m80-firecracker/benches/sweep-mem_mib.csv`
- `docs/perf/mem-sizing.md`

Required interpretation: phase_12b_ready_accept P50/P95 by memory size,
snapshot file size by memory size, density memory commitment at C=8, and a
plain workload-coverage rationale for the chosen default.

## E4. Loaded-Host Bench

Gates: `m80-jp6ik.25`.

Purpose: quantify the addressable host-scheduler headroom before considering
boot-phase `SCHED_FIFO` elevation.

Invocation:

```sh
STRESS_PROCS="$(nproc)" N=50 KIND=minimal ./scripts/bench-cold-launch.sh
```

If a dedicated background-stress mode is added, record the exact knob here and
in `docs/perf/loaded-host.md`.

Artifacts:

- `crates/m80-firecracker/benches/snapshots/loaded-host-N50.json`
- `docs/perf/loaded-host.md`

Required interpretation: idle vs loaded phase_12b_ready_accept P50/P95/P99,
success rate, failure signatures if any, and the percentage of loaded-host delta
that a scheduler experiment must recover to justify the complexity.

## E5. TLB And Cache-Miss Counters

Gates: `m80-jp6ik.10`, `m80-jp6ik.27`.

Purpose: decide whether hugepages and CPU-template changes have enough
phase_12b headroom to justify implementation.

Invocation:

```sh
PERF_STAT=1 N=20 KIND=minimal SKIP_LOADED=1 ./scripts/bench-cold-launch.sh
```

Host prerequisites: `perf` installed and `sudo perf stat` permitted. On hosts
with `kernel.perf_event_paranoid > 1`, the harness still works when sudo grants
the needed perf capability.

Artifacts:

- `crates/m80-firecracker/benches/perf-counters.csv`
- `docs/perf/tlb-pressure.md`

Required interpretation: per-launch dTLB-load-misses, iTLB-load-misses, and
cache-misses histogram during phase_12b. If dTLB-load-misses are below 100K per
launch, the hugepages payoff is bounded. If they exceed 1M per launch, there is
meaningful headroom.

## E6. Dirty-Page Fraction

Gates: `m80-jp6ik.19`.

Purpose: measure whether diff snapshots are materially smaller than full
snapshots after pristine boot and after a representative exec.

Invocation:

```sh
./scripts/bench-dirty-page-fraction.sh
```

This experiment depends on the `track_dirty_pages` machine-config field
existing. If that field is scaffolded as part of the diff-snapshot bead, keep
the scaffolded implementation open until the real dirty-fraction artifact is
committed.

Artifacts:

- `crates/m80-firecracker/benches/dirty-fraction.csv`
- `docs/perf/dirty-page-baseline.md`

Required interpretation: full snapshot size, diff snapshot size, dirty pages,
total pages, and dirty fraction for pristine post-ready and representative
post-exec states.

## E7. CPU Governor Sweep

Gates: `m80-jp6ik.40`.

Purpose: distinguish hosts where CPU governor tuning matters
(`acpi-cpufreq` with non-performance governor) from hosts where hardware P-state
management makes the advisory irrelevant.

Invocation:

```sh
CPU_GOVERNOR=performance N=50 KIND=minimal SKIP_LOADED=1 \
  ./scripts/bench-cold-launch.sh
CPU_GOVERNOR=ondemand N=50 KIND=minimal SKIP_LOADED=1 \
  ./scripts/bench-cold-launch.sh
```

Artifacts:

- `crates/m80-firecracker/benches/snapshots/cpu-governor-sweep.json`
- `docs/perf/cpu-governor.md`

Required interpretation: scaling driver, governor, phase_12b_ready_accept
P50/P95/P99, and whether the host is an `acpi-cpufreq` host where advisory text
should fire.

## E8. Tokio Runtime Wallclock

Gates: `m80-jp6ik.31`.

Purpose: measure whether per-launch Tokio current-thread runtime construction
is large enough to justify replacing async rtnetlink with synchronous netlink.

Invocation:

```sh
PHASE_JSONL=crates/m80-firecracker/benches/tokio-runtime-cost.jsonl \
  EGRESS=outbound N=20 KIND=minimal SKIP_LOADED=1 ./scripts/bench-cold-launch.sh
```

This requires temporary instrumentation in:

- `crates/m80-guestd/src/pid_one_network.rs`
- `crates/m80-net-outbound/src/link_ops.rs`

Artifacts:

- `crates/m80-firecracker/benches/tokio-runtime-cost.jsonl`
- `docs/perf/tokio-runtime-cost.md`

Required interpretation: P50/P95 for each runtime-construction call site. If
the measured cost is below 3 ms, reconsider the replacement bead. If it exceeds
7 ms, the replacement is justified.

## E9. Kernel Cmdline And Config Delta

Gates: `m80-jp6ik.5`, `m80-jp6ik.6`, `m80-jp6ik.23`.

Purpose: attribute cold-boot wins from stripped-kernel command-line and Kconfig
changes without keeping zero-gain kernel knobs.

Invocation:

```sh
N=50 KIND=minimal SKIP_LOADED=1 KERNEL_KIND=stock ./scripts/bench-cold-launch.sh
N=50 KIND=minimal SKIP_LOADED=1 KERNEL_KIND=stripped ./scripts/bench-cold-launch.sh
```

For staged comparisons, run one cell per variant:

- baseline stripped kernel
- `nokaslr nosmp maxcpus=0`
- `CONFIG_SMP=n` plus stripped debug sections
- the additional HZ, preemption, initrd, scheduler-debug, debug-info, and
  printk-time settings

Artifacts:

- `crates/m80-firecracker/benches/snapshots/kernel-boot-delta.json`
- `docs/perf/kernel-cmdline.md`
- `docs/perf/kernel-config-smp.md`
- `docs/perf/kernel-config-additional.md`

Required interpretation: phase_12b_ready_accept P50/P95/P99 per variant,
kernel size before/after, boot success rate, and per-subphase attribution when
guest boot decomposition is available. Roll back any flag or config setting
that does not measure positive.

## E10. Cargo Release Profile Delta

Gates: `m80-jp6ik.18`.

Purpose: prove that release-profile tuning reduces binary size without hiding a
runtime regression in launch-path code.

Invocation:

```sh
cargo build --release -p m80-cli
N=50 KIND=minimal SKIP_LOADED=1 ./scripts/bench-cold-launch.sh
```

Run the same commands before and after the profile change. Keep test builds on
`panic = "unwind"` so test diagnostics do not degrade.

Artifacts:

- `crates/m80-firecracker/benches/snapshots/release-profile-delta.json`
- `docs/perf/release-profile.md`

Required interpretation: `target/release/m80` size before/after, release build
success, test-profile panic behavior, and phase_1_run_root_prep plus wallclock
P50/P95 before/after. A binary-size win alone is useful, but any runtime
regression must be named.

## E11. KVM Halt-Poll Advisory

Gates: `m80-jp6ik.30`.

Purpose: keep the KVM host-tuning bead scoped as visibility unless a real
measurement later promotes it to a latency claim.

Invocation:

```sh
m80 preflight
```

Optional measurement, only if the bead is promoted from advisory to measured
latency work:

```sh
N=50 KIND=minimal SKIP_LOADED=1 ./scripts/bench-cold-launch.sh
```

Artifacts:

- `docs/ops/host-tuning.md`
- optional: `docs/perf/kvm-halt-poll.md`

Required interpretation: preflight surfaces the current
`/sys/module/kvm/parameters/halt_poll_ns` value and the ops doc explains
latency-priority versus density-priority settings. Do not claim a measured
latency win without a committed before/after bench artifact.

## E12. CPU Template Delta

Gates: `m80-jp6ik.27`.

Purpose: measure the small phase_12a/early-boot effect of removing the default
Intel `T2` CPU template while preserving same-host snapshot behavior.

Invocation:

```sh
N=50 KIND=minimal SKIP_LOADED=1 ./scripts/bench-cold-launch.sh
```

Artifacts:

- `crates/m80-firecracker/benches/snapshots/cpu-template-delta.json`
- `docs/perf/cpu-template.md`

Required interpretation: machine-config PUT omits `cpu_template` by default,
phase_12a_instance_start P50 before/after, same-host snapshot/restore result,
and the documented tradeoff that future cross-host restore needs explicit
CPU-feature parity verification.

## E13. Composed E2E

Gates: `m80-q420k.6.2`, `m80-q420k.6.3`, `m80-q420k.6.4`,
`m80-q420k.6.5`.

Purpose: prove that the composed layered-rootfs path survives real-KVM
snapshot restore with a Shared pmem layer, a PerVm pmem layer, post-restore
hooks, bounded host-memory growth, and zero teardown residue.

Invocation:

```sh
sudo -n env \
  PATH="$PATH" HOME="$HOME" CARGO_HOME="$HOME/.cargo" RUSTUP_HOME="$HOME/.rustup" \
  M80_COMPOSED_E2E_ALLOW_OTHER_VMS=0 \
  M80_COMPOSED_E2E_N=10 \
  M80_COMPOSED_E2E_SHARED_PAYLOAD_MIB=32 \
  M80_COMPOSED_E2E_OUT_DIR=/tank/projects/m80/crates/m80-firecracker/benches/snapshots \
  M80_RUN_ROOT=/var/lib/m80-composed-e2e \
  M80_JAIL_UID="$(id -u)" \
  M80_JAIL_GID="$(id -g)" \
  M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker \
  M80_JAILER_BIN=/opt/firecracker/bin/jailer \
  M80_FIRECRACKER_SECCOMP_FILTER=/opt/firecracker/bin/firecracker-seccomp-filter.bin \
  M80_JAILER_HARDEN_BIN=/opt/m80/bin/m80-jailer-harden \
  M80_NET_HELPER_BIN=/opt/m80/bin/m80-net-helper \
  M80_ROOTFS_IMAGE=<real-rootfs.ext4> \
  M80_KERNEL_IMAGE=<real-stripped-kernel.bin> \
  M80_KERNEL_KIND=stripped \
  cargo test --release -p m80-firecracker --test e2e_composed_real_kvm -- \
    --ignored composed_e2e_layered_warm_pool --nocapture
```

Artifacts:

- `crates/m80-firecracker/benches/snapshots/composed-e2e-restore-N10.json`
- `crates/m80-firecracker/benches/snapshots/composed-e2e-host-memory.json`
- `crates/m80-firecracker/benches/snapshots/composed-e2e-residue.json`
- `docs/perf/composed-e2e.md`

Required interpretation: restore P50/P95/P99 with `p99_ms <= 200`,
`page_cache_dropped_between_leases=false`, host-memory delta compared to a
same-run PerVm baseline bound,
`page_cache_dropped_between_fill_and_attach=false`, the Shared payload's
`.8.12`-proven erofs layout (`dump.erofs --path=/payload.bin` reports
`Layout: 0` and equal logical/on-disk size), residue scan roots covering the
explicit run root, `/tmp/m80-*`, `/var/run/m80`, image store, and template store,
leased-run-dir enumeration as absolute path strings, empty unexpected-path
result, exact image-store preservation, exact template-store preservation,
empty post-run Firecracker process list, preflight artifact identity, measured
git commit, exact N=10 cardinality, runtime host kernel >= 6.5, `/dev/kvm` rw
stat output, sudo uid `0`, actual Firecracker version matching preflight,
host-memory bound formula `shared_image_bytes + per_vm_overhead_bytes *
n_attached`, image-store expected set exactly equal to the Shared and PerVm
digests, lowercase sha256 Shared and PerVm image-store digests, and lowercase
sha256 template-store fingerprints. The restore artifact's `samples_ms` array
must match `count`, and
`template_build_warmup_ms` must be non-empty. Its `target_ready` must equal
`count`, P50/P95/P99 values must recompute from `samples_ms`, warm-pool
snapshots must prove all 10 slots filled and then all 10 slots leased, and
diagnostics must show restore/load/probe/post-restore-hook phases plus workload
exec completions for every lease. The host-memory artifact must carry raw
MemAvailable checkpoints that derive `after_n_attached_delta_bytes`, and its
Shared image path must contain the Shared digest. The same-run PerVm baseline
section must agree with the top-level Shared digest, Shared bytes, attached
count, payload-copy bytes, attached-memory delta, derived per-VM overhead, and
all-slots-leased snapshot. The receipt doc must also retain the exact
`cargo test --release -p m80-firecracker --test e2e_composed_real_kvm`
reproduction command, host context, page-cache statement, and green
`composed_e2e_layered_warm_pool` test-result lines, with exactly one
`M80_COMPOSED_E2E_ARTIFACT` smoke line for each canonical composed JSON
artifact. The verifier also checks that the three composed JSON artifacts agree
on `git_commit`, `substrate`, target count, and lowercase sha256 Shared image
digest, and that the receipt doc mentions the measured git commit, substrate
identity, Shared image digest, restore P50/P95/P99, and host-memory byte values
from those JSON artifacts. Run the composed
JSON subset after committing artifacts and before closing `m80-q420k.6.2`,
`.6.3`, and `.6.4`:

```sh
python3 scripts/verify-q420k-artifacts.py \
  --only composed-restore --only composed-memory --only composed-residue \
  --require-committed
```

Run the verifier without `--only` before closing `.6.5` or the parent
super-epic; that adds the `docs/perf/composed-e2e.md` receipt check. The
`.6.5` close reason must reference the three JSON measurement `verified:`
lines, using the same commits as the already-closed `.6.2`, `.6.3`, and
`.6.4` close reasons, as well as the receipt doc's own verified line. For
final parent close, use the full gate:

```sh
python3 scripts/verify-q420k-artifacts.py \
  --require-committed \
  --require-closed-beads \
  --require-parent-phases-closed
```

## Attribution Rules

Every gated bead that cites this playbook should name the experiment section in
its acceptance criteria, for example:

```text
Verified per docs/perf/measurement-playbook.md#e3-mem-size-sweep:
docs/perf/mem-sizing.md @ <commit>.
```

The section link answers "what protocol produced this number". The artifact
path answers "where is the number". The commit answers "which repo state
produced and interpreted it".
