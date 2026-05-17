# Pmem DAX Memory Pressure

Bead: `m80-q420k.8.9`.

## Substrate

- substrate kind: `real-kvm`
- commit: `1c63e8ef3ecc8a3a5e141ce5293f3741c45c201a`
- git worktree dirty excluding this artifact: `false`
- host kernel: `6.17.0-23-generic`
- host filesystem: `ext4`
- host memory: `197896945664 bytes MemTotal`
- pressure command: `stress-ng --vm 1 --vm-bytes 85% --vm-keep --timeout 60s`
- pressure settle ms: `10000`
- VM count: `2`
- samples per guest: `12`
- latency timing source: `host monotonic Instant around one guest dd exec per sample (includes exec/vsock overhead)`
- payload size: `256 MiB`
- image digest: `ce5ad6287bdb114fbf9faaa6dbda3cdf512e970d26444bbba987241f5ff24f5d`
- image path: `/var/lib/m80-images/ce/ce5ad6287bdb114fbf9faaa6dbda3cdf512e970d26444bbba987241f5ff24f5d/image.erofs`
- payload erofs layout: `Layout: 0`, size `268435456` bytes, on-disk size `268435456` bytes, compression ratio `100.00%`
- Firecracker version: `Firecracker v1.15.1; 2026-05-17T12:47:03.907360572 [anonymous-instance:main] Firecracker exiting successfully. exit_code=0`
- unrelated VMs running: `false`
- command:

```sh
M80_RUN_PMEM_DAX_MEMORY_PRESSURE=1 M80_PMEM_DAX_MEMORY_PRESSURE_COMMAND='stress-ng --vm 1 --vm-bytes 85% --vm-keep --timeout 60s' M80_PMEM_DAX_MEMORY_PRESSURE_VM_COUNT=2 M80_PMEM_DAX_MEMORY_PRESSURE_SAMPLES=12 M80_PMEM_DAX_MEMORY_PRESSURE_PAYLOAD_MIB=256 M80_PMEM_DAX_MEMORY_PRESSURE_SETTLE_MS=10000 M80_PMEM_DAX_MEMORY_PRESSURE_ARTIFACT=docs/perf/pmem-dax-memory-pressure.md M80_RUN_ROOT=/var/lib/m80-dax-memory-pressure M80_JAIL_UID=1000 M80_JAIL_GID=1000 M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker M80_JAILER_BIN=/opt/firecracker/bin/jailer M80_FIRECRACKER_SECCOMP_FILTER=/opt/firecracker/bin/firecracker-seccomp-filter.bin M80_JAILER_HARDEN_BIN=/opt/m80/bin/m80-jailer-harden M80_NET_HELPER_BIN=/opt/m80/bin/m80-net-helper M80_KERNEL_IMAGE=/tank/projects/m80/crates/m80-image-build/kernels/vmlinux-m80-613988fdb6a6aaa0f806ec27e6f6e66875e2f768b28a2c0244aa4af3d8e2ac19.bin M80_KERNEL_KIND=stripped M80_ROOTFS_IMAGE=/tank/tmp/m80-build/post-restore-current/output.ext4 cargo test -p m80-firecracker --test pmem_dax_memory_pressure_real_kvm -- --ignored --nocapture
```

### Firecracker process substrate

```json
{
  "allow_other_firecracker_vms": false,
  "dev_kvm_stat": "crw-rw---- root:kvm /dev/kvm",
  "firecracker_version": "Firecracker v1.15.1\n\n2026-05-17T12:45:49.701044571 [anonymous-instance:main] Firecracker exiting successfully. exit_code=0",
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

- baseline Shared-payload read latency p50_ms: `39.994`
- baseline Shared-payload read latency p95_ms: `173.251`
- baseline Shared-payload read latency p99_ms: `173.251`
- post-pressure Shared-payload read latency p50_ms: `39.986`
- post-pressure Shared-payload read latency p95_ms: `40.029`
- post-pressure Shared-payload read latency p99_ms: `40.029`
- cross-guest signal delta p50_ms: `0.000`
- host memory delta during pressure bytes: `24031440896`
- host page-cache delta during pressure bytes: `0`
- host memory delta after refault bytes: `0`
- host page-cache delta after refault bytes: `0`

### Per-guest Latency Samples

- baseline guest 0 sorted_samples_ms: `[39.800, 39.813, 39.915, 39.947, 39.975, 39.994, 39.998, 40.008, 40.021, 40.195, 40.204, 196.612]`
- baseline guest 1 sorted_samples_ms: `[39.834, 39.952, 39.967, 39.980, 39.994, 40.006, 40.009, 40.020, 40.024, 40.024, 40.042, 173.251]`
- post-pressure guest 0 sorted_samples_ms: `[39.933, 39.973, 39.977, 39.978, 39.991, 39.992, 39.998, 39.998, 40.017, 40.029, 40.044, 41.218]`
- post-pressure guest 1 sorted_samples_ms: `[33.104, 39.971, 39.977, 39.980, 39.986, 39.986, 40.002, 40.008, 40.022, 40.025, 40.026, 40.029]`

## Teardown Residue

- leaked Shared markers: `0`
- leaked Firecracker/jailer processes: `0`
- leaked mounts: `0`

## Decision Output

On this real-KVM run, the probe used host monotonic timing around each guest `dd` read command, so the numbers include exec/vsock overhead but are not quantized by the guest clock. The run used two Shared-pmem guests, a 256 MiB uncompressed erofs DAX payload, 12 samples per guest, `stress-ng --vm 1 --vm-bytes 85% --vm-keep --timeout 60s`, and a 10s pressure settle window. Baseline Shared-payload read latency was p50 `39.994 ms`, p95 `173.251 ms`, p99 `173.251 ms`; post-pressure latency was p50 `39.986 ms`, p95 `40.029 ms`, p99 `40.029 ms`; and the p50 cross-guest signal delta was `0.000 ms`. Host `MemAvailable` dropped by `24031440896` bytes during the settled pressure sample, and teardown residue was zero. Under the same-trust-domain Shared pmem assumption, this substrate does not justify a q420k runtime mitigation such as `mlock`, `MAP_POPULATE`, or `madvise(MADV_WILLNEED)`. It does not remove the side-channel boundary: Shared pmem remains same-trust-domain only.
