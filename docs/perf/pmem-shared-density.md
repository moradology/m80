# Shared pmem density

Generated: `2026-05-17T11:25:33+00:00`

## Reproduction

Command: `M80_PMEM_SHARED_ALLOW_OTHER_VMS=0 M80_PMEM_SHARED_VM_COUNT=4 M80_PMEM_SHARED_CYCLES=10 M80_PMEM_SHARED_PAYLOAD_MIB=128 M80_PMEM_SHARED_PER_VM_OVERHEAD_KIB=131072 M80_PMEM_SHARED_DENSITY_ARTIFACT=docs/perf/pmem-shared-density.md M80_RUN_ROOT=/var/lib/m80-psd M80_KERNEL_IMAGE=/tank/projects/m80/crates/m80-image-build/kernels/vmlinux-m80-613988fdb6a6aaa0f806ec27e6f6e66875e2f768b28a2c0244aa4af3d8e2ac19.bin M80_KERNEL_KIND=stripped M80_ROOTFS_IMAGE=/tank/tmp/m80-build/post-restore-current/output.ext4 M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker M80_JAILER_BIN=/opt/firecracker/bin/jailer M80_FIRECRACKER_SECCOMP_FILTER=/opt/firecracker/bin/firecracker-seccomp-filter.bin M80_JAILER_HARDEN_BIN=/opt/m80/bin/m80-jailer-harden M80_NET_HELPER_BIN=/opt/m80/bin/m80-net-helper M80_FIRECRACKER_VERSION=v1.15.1 M80_JAIL_UID=1000 M80_JAIL_GID=1000 ./scripts/smoke-pmem-shared.sh`

## Substrate

- host kernel: `6.17.0-23-generic`
- firecracker: `Firecracker v1.15.1; 2026-05-17T11:25:33.540562298 [anonymous-instance:main] Firecracker exiting successfully. exit_code=0`
- `/dev/kvm`: `crw-rw---- root:kvm /dev/kvm`
- sudo: required; test ran as uid `0`
- dropped page cache before each cycle: `sync && echo 3 > /proc/sys/vm/drop_caches`
- quiet host check: `true`
- allow other Firecracker VMs: `false`
- git worktree dirty excluding this artifact: `false`
- git commit: `3706f497977e7a1e64bba6309d113aad2dde485b`

### Firecracker process substrate

```json
{
  "allow_other_firecracker_vms": false,
  "dev_kvm_stat": "crw-rw---- root:kvm /dev/kvm",
  "firecracker_version": "Firecracker v1.15.1\n\n2026-05-17T11:24:36.429593749 [anonymous-instance:main] Firecracker exiting successfully. exit_code=0",
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

- field: host memory delta after 4 attached Shared VMs
- cycles: `10`
- payload size: `128 MiB`
- image digest: `a691af5c894244b7c248aeb2068705a801094a2ca2ecf6b6dd26d63d23775fe5`
- image path: `/var/lib/m80-images/a6/a691af5c894244b7c248aeb2068705a801094a2ca2ecf6b6dd26d63d23775fe5/image.erofs`
- image bytes: `134221824`
- image KiB: `131076`
- payload erofs layout: `Layout: 0`, size `134217728` bytes, on-disk size `134217728` bytes, compression ratio `100.00%`
- per-VM overhead bound: `131072 KiB`
- bound: `655364 KiB`
- max observed delta: `252308 KiB`
- result: `pass`

## Teardown

- max active-use markers observed: `4`
- final active-use markers: `0`
- stale markers swept after teardown: `0`
- canonical Shared artifact present after teardown: `true`

## Samples

| cycle | MemAvailable before KiB | MemAvailable after KiB | delta KiB | bound KiB |
|---:|---:|---:|---:|---:|
| 1 | 151377152 | 151170660 | 206492 | 655364 |
| 2 | 151317436 | 151130760 | 186676 | 655364 |
| 3 | 151241916 | 151088624 | 153292 | 655364 |
| 4 | 151257924 | 151005616 | 252308 | 655364 |
| 5 | 151161332 | 150948676 | 212656 | 655364 |
| 6 | 151115956 | 150973452 | 142504 | 655364 |
| 7 | 151143552 | 151004168 | 139384 | 655364 |
| 8 | 151152952 | 150978340 | 174612 | 655364 |
| 9 | 151133496 | 150985252 | 148244 | 655364 |
| 10 | 151144608 | 150902892 | 241716 | 655364 |

## Payload erofs layout

The measured file is required to match the `.8.12` file-level DAX result: uncompressed, non-inlined erofs layout 0.

```text
Path : /payload.bin
Size: 134217728  On-disk size: 134217728  regular file
NID: 40   Links: 1   Layout: 0   Compression ratio: 100.00%
Inode size: 32   Xattr size: 0
Uid: 0   Gid: 0  Access: 0644/rw-r--r--
Timestamp: 1970-01-01 00:00:00.000000000
```

## Trust model

Shared pmem is admitted only with `TrustDomainAck` in the same trust domain.
The DAX cache-timing side channel is acknowledged by that trust model.
Shared pmem jail bindings are read-only; writable layers must use `PmemSharing::PerVm`.

