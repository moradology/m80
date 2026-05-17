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
Close-quality JSON also records runtime substrate values: host kernel release,
actual Firecracker version, `/dev/kvm` stat output, and sudo uid. The verifier
requires host kernel >= 6.5, `/dev/kvm` writable, sudo uid `0`, and actual
Firecracker version matching preflight.

## Artifact identity

- git_commit: `49d991f2ce1013501641d48d70b9612eda0fb5db`
- host_kernel_release: `6.17.0-23-generic`
- firecracker_version:

```text
Firecracker v1.15.1

2026-05-17T11:31:56.764192212 [anonymous-instance:main] Firecracker exiting successfully. exit_code=0
```

- dev_kvm_stat: `crw-rw---- root:kvm /dev/kvm`
- sudo_uid: `0`
- firecracker_bin: `/opt/firecracker/bin/firecracker`
- firecracker_seccomp_filter: `/opt/firecracker/bin/firecracker-seccomp-filter.bin`
- jailer_bin: `/opt/firecracker/bin/jailer`
- jailer_harden_bin: `/opt/m80/bin/m80-jailer-harden`
- net_helper_bin: `/opt/m80/bin/m80-net-helper`
- kernel_image: `/tank/projects/m80/crates/m80-image-build/kernels/vmlinux-m80-613988fdb6a6aaa0f806ec27e6f6e66875e2f768b28a2c0244aa4af3d8e2ac19.bin`
- rootfs_image: `/tank/tmp/m80-build/post-restore-current/output.ext4`
- kernel_image_sha256: `143b2784a434cdf5de10920a59e2c875b66be63bacfa9ba8ddf93ac60f2bc6e3`
- rootfs_image_sha256: `bfa35731760b9fbf06d41ffbfe500153d3247869dee75443dc836184b253613d`
- kernel_kind: `stripped`
- image_kind: `minimal`
- rootfs_format: `ext4`
- expected_firecracker_version: `v1.15.1`

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
post-run Firecracker leak marker, preflight artifact identity, runtime
substrate fields, committed git state, and the committed command/close-reason
text above. It also requires
exact `n_per_run=20`, `runs=3`, and `samples_total=60`, a full 40-character
`git_commit`, `samples_us` and `sample_details` arrays whose lengths match
`samples_total`, one `runs_detail` entry per run, exact `sample_details`
coverage for every `(run, cycle)` in the N x runs matrix, a `p99` value
recomputed from those samples, and a close-quality smoke paste in the section
below. It does not replace the verified-close reason.

## Smoke evidence

```text
snapshot-template restore: load=idle runs=3 n=20 p99=156585us output=crates/m80-firecracker/benches/snapshot_template_restore_latency.json
```
