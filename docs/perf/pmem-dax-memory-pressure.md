# Pmem DAX Memory Pressure

Bead: `m80-q420k.8.9`.

## Substrate

- substrate kind: `real-kvm`
- commit: `13bf4697449f10bcffe28ee1915587b3f28cd9d3`
- git worktree dirty excluding this artifact: `false`
- host kernel: `6.17.0-23-generic`
- host filesystem: `ext4`
- host memory: `197896945664 bytes MemTotal`
- pressure command: `stress-ng --vm 1 --vm-bytes 50% --timeout 30s`
- VM count: `2`
- samples per guest: `5`
- payload size: `32 MiB`
- image digest: `51453c4b9046488a66c1b5362466fc123f74bbf9a56284af4dc189e47775b2dd`
- image path: `/var/lib/m80-images/51/51453c4b9046488a66c1b5362466fc123f74bbf9a56284af4dc189e47775b2dd/image.erofs`
- payload erofs layout: `Layout: 0`, size `33554432` bytes, on-disk size `33554432` bytes, compression ratio `100.00%`
- Firecracker version: `Firecracker v1.15.1; 2026-05-17T12:29:43.532139694 [anonymous-instance:main] Firecracker exiting successfully. exit_code=0`
- unrelated VMs running: `false`
- command:

```sh
M80_RUN_PMEM_DAX_MEMORY_PRESSURE=1 M80_PMEM_DAX_MEMORY_PRESSURE_COMMAND='stress-ng --vm 1 --vm-bytes 50% --timeout 30s' M80_PMEM_DAX_MEMORY_PRESSURE_VM_COUNT=2 M80_PMEM_DAX_MEMORY_PRESSURE_SAMPLES=5 M80_PMEM_DAX_MEMORY_PRESSURE_PAYLOAD_MIB=32 M80_PMEM_DAX_MEMORY_PRESSURE_ARTIFACT=docs/perf/pmem-dax-memory-pressure.md M80_RUN_ROOT=/var/lib/m80-dax-memory-pressure M80_JAIL_UID=1000 M80_JAIL_GID=1000 M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker M80_JAILER_BIN=/opt/firecracker/bin/jailer M80_FIRECRACKER_SECCOMP_FILTER=/opt/firecracker/bin/firecracker-seccomp-filter.bin M80_JAILER_HARDEN_BIN=/opt/m80/bin/m80-jailer-harden M80_NET_HELPER_BIN=/opt/m80/bin/m80-net-helper M80_KERNEL_IMAGE=/tank/projects/m80/crates/m80-image-build/kernels/vmlinux-m80-613988fdb6a6aaa0f806ec27e6f6e66875e2f768b28a2c0244aa4af3d8e2ac19.bin M80_KERNEL_KIND=stripped M80_ROOTFS_IMAGE=/tank/tmp/m80-build/post-restore-current/output.ext4 cargo test -p m80-firecracker --test pmem_dax_memory_pressure_real_kvm -- --ignored --nocapture
```

### Firecracker process substrate

```json
{
  "allow_other_firecracker_vms": false,
  "dev_kvm_stat": "crw-rw---- root:kvm /dev/kvm",
  "firecracker_version": "Firecracker v1.15.1\n\n2026-05-17T12:29:02.829610139 [anonymous-instance:main] Firecracker exiting successfully. exit_code=0",
  "host_kernel_release": "6.17.0-23-generic",
  "post_run_firecracker_processes": [],
  "preexisting_firecracker_processes": [],
  "preflight_artifacts": {
    "expected_firecracker_version": "v1.15.1",
    "firecracker_bin": "/opt/firecracker/bin/firecracker",
    "firecracker_seccomp_filter": "/opt/firecracker/bin/firecracker-seccomp-filter.bin",
    "image_kind": "minimal",
    "jailer_bin": "/opt/firecracker/bin/jailer",
    "jailer_harden_bin": "/opt/m80/bin/m80-jailer-harden",
    "kernel_image": "/tank/projects/m80/crates/m80-image-build/kernels/vmlinux-m80-613988fdb6a6aaa0f806ec27e6f6e66875e2f768b28a2c0244aa4af3d8e2ac19.bin",
    "kernel_image_sha256": "143b2784a434cdf5de10920a59e2c875b66be63bacfa9ba8ddf93ac60f2bc6e3",
    "kernel_kind": "stripped",
    "net_helper_bin": "/opt/m80/bin/m80-net-helper",
    "rootfs_format": "ext4",
    "rootfs_image": "/tank/tmp/m80-build/post-restore-current/output.ext4",
    "rootfs_image_sha256": "bfa35731760b9fbf06d41ffbfe500153d3247869dee75443dc836184b253613d"
  },
  "preflight_required": true,
  "quiet_host_checked": true,
  "substrate_kind": "real-kvm",
  "sudo_uid": "0"
}
```

## Observable

- baseline Shared-payload read latency p50_ms: `10.000`
- baseline Shared-payload read latency p95_ms: `40.000`
- baseline Shared-payload read latency p99_ms: `40.000`
- post-pressure Shared-payload read latency p50_ms: `10.000`
- post-pressure Shared-payload read latency p95_ms: `10.000`
- post-pressure Shared-payload read latency p99_ms: `10.000`
- cross-guest signal delta p50_ms: `0.000`
- host memory delta during pressure bytes: `38084608`
- host page-cache delta during pressure bytes: `0`
- host memory delta after refault bytes: `615653376`
- host page-cache delta after refault bytes: `0`

## Teardown Residue

- leaked Shared markers: `0`
- leaked Firecracker/jailer processes: `0`
- leaked mounts: `0`

## Decision Output

On this real-KVM run, baseline Shared-payload read latency was p50 `10.000 ms`, p95 `40.000 ms`, p99 `40.000 ms`; post-pressure latency was p50 `10.000 ms`, p95 `10.000 ms`, p99 `10.000 ms`; and the p50 cross-guest signal delta was `0.000 ms`. Under the same-trust-domain Shared pmem assumption, this signal is acceptable for q420k. No runtime mitigation bead for `mlock`, `MAP_POPULATE`, or `madvise(MADV_WILLNEED)` is filed from this run; the residual risk remains documented as a same-trust-domain-only Shared-pmem tradeoff.
