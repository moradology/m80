# Snapshot-Template Restore Latency

This measurement closes `m80-q420k.4.15` only when run on a quiet real-KVM
host and committed as the named artifact. Bench compilation or mock execution
does not satisfy the bead.

## Observable

- Artifact:
  `crates/m80-firecracker/benches/snapshot_template_restore_latency.json`
- Field: `data.warm.restore_to_handback_ms.p99`
- Bound: `<= 200.0`
- Target ready count: `target_ready=1`
- Substrate: real KVM, `/dev/kvm` writable, sudo available, no other admitted
  VMs, `M80_SNAPSHOT_BENCH_LOAD=idle`
- Samples: `N=20` lease cycles per run, `M80_SNAPSHOT_TEMPLATE_RUNS=3`,
  producing 60 merged samples.

The bench records the warm-pool fill duration from immediately before
`launch_slot` work starts until the restored slot enters the ready queue, then
adds the immediate `WarmPool::try_lease` handoff duration. For
`WarmStrategy::SnapshotRestore`, the fill duration includes template
lookup/build, snapshot restore, post-restore hooks, and ready probes. The first
cache-miss template build is recorded separately and excluded from the 60
steady-state samples.

The bench refuses to run when pre-existing Firecracker processes are present.
`M80_SNAPSHOT_TEMPLATE_ALLOW_OTHER_VMS=1` permits a non-closeable diagnostic
run; the JSON records the override and the pre-existing Firecracker processes
under `substrate`. Close-quality JSON also records an empty
`substrate.post_run_firecracker_processes` list after the measured runs finish
and a `substrate.preflight_artifacts` object naming the resolved
Firecracker/jailer/kernel/rootfs inputs and manifest sha256s. The JSON also
records the full measured `git_commit` and a `reproduction_command` that
includes the measured Firecracker, jailer, helper, kernel, rootfs, run-root,
jail UID/GID, and disabled cgroup-mode inputs.

## Command

From the repository root:

```sh
sync && echo 3 | sudo tee /proc/sys/vm/drop_caches

sudo -n env PATH=/home/nathan/.cargo/bin:/usr/sbin:/usr/bin:/sbin:/bin \
  CARGO_TARGET_DIR=/tank/tmp/m80-sudo-target \
  M80_SNAPSHOT_TEMPLATE_ALLOW_OTHER_VMS=0 \
  M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker \
  M80_JAILER_BIN=/opt/firecracker/bin/jailer \
  M80_FIRECRACKER_SECCOMP_FILTER=/opt/firecracker/bin/firecracker-seccomp-filter.bin \
  M80_JAILER_HARDEN_BIN=/opt/m80/bin/m80-jailer-harden \
  M80_NET_HELPER_BIN=/opt/m80/bin/m80-net-helper \
  M80_KERNEL_IMAGE=/tank/projects/m80/crates/m80-image-build/kernels/vmlinux-m80-613988fdb6a6aaa0f806ec27e6f6e66875e2f768b28a2c0244aa4af3d8e2ac19.bin \
  M80_KERNEL_KIND=stripped \
  M80_ROOTFS_IMAGE=/tank/tmp/m80-build/post-restore-current/output.ext4 \
  M80_RUN_ROOT=/tank/tmp/m80-template-build-test \
  M80_SNAPSHOT_BENCH_VCPU_COUNT=1 M80_SNAPSHOT_BENCH_MEM_SIZE_MIB=512 \
  M80_JAIL_UID=1000 M80_JAIL_GID=1000 M80_CGROUP_MODE=disabled \
  M80_SNAPSHOT_BENCH_LOAD=idle \
  N=20 M80_SNAPSHOT_TEMPLATE_RUNS=3 \
  M80_SNAPSHOT_TEMPLATE_BENCH_OUTPUT=crates/m80-firecracker/benches/snapshot_template_restore_latency.json \
  /home/nathan/.cargo/bin/cargo bench -p m80-firecracker --bench snapshot_template_restore_latency
```

The close reason must cite:

```text
verified: crates/m80-firecracker/benches/snapshot_template_restore_latency.json @ <commit-sha>
```

Before closing `m80-q420k.4.15`, after committing the artifact, run:

```sh
python3 scripts/verify-q420k-artifacts.py --only snapshot-template --require-committed
```

The `--only snapshot-template` selector also checks this reproduction doc.
The verifier checks the named JSON fields, quiet-host substrate markers,
post-run Firecracker leak marker, preflight artifact identity, committed git
state, and the committed command/close-reason text above. It also requires a
full 40-character `git_commit`, `samples_us` and `sample_details` arrays whose
lengths match `samples_total`, one `runs_detail` entry per run, exact
`sample_details` coverage for every `(run, cycle)` in the N x runs matrix, a
`p99` value recomputed from those samples, and a close-quality smoke paste in
the section below. It does not replace the verified-close reason.

## Smoke evidence

Pending quiet-host close run. Replace this section with the bench stderr line
from the same run that wrote
`crates/m80-firecracker/benches/snapshot_template_restore_latency.json`.

## Diagnostic Run

On 2026-05-17, the full N=20 x 3 harness completed on this host with
`data.warm.restore_to_handback_ms.p99 = 148.713` and
`samples_total = 60`. The diagnostic output was written to
`/tmp/m80-snapshot-template-restore-latency-diagnostic-N20x3.json`.

This does not close `m80-q420k.4.15`: unrelated
`t2-warm-slot-*` Firecracker processes from
`/tank/torpor-run/sandbox-executor/m80` were present, and the artifact was not
committed at the verified-close path.
