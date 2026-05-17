# Shared pmem density

Generated: `2026-05-17T11:18:22+00:00`

## Reproduction

Command: `M80_PMEM_SHARED_ALLOW_OTHER_VMS=0 M80_PMEM_SHARED_VM_COUNT=4 M80_PMEM_SHARED_CYCLES=10 M80_PMEM_SHARED_PAYLOAD_MIB=128 M80_PMEM_SHARED_PER_VM_OVERHEAD_KIB=131072 M80_PMEM_SHARED_DENSITY_ARTIFACT=docs/perf/pmem-shared-density.md M80_RUN_ROOT=/var/lib/m80-psd M80_KERNEL_IMAGE=/tank/projects/m80/crates/m80-image-build/kernels/vmlinux-m80-613988fdb6a6aaa0f806ec27e6f6e66875e2f768b28a2c0244aa4af3d8e2ac19.bin M80_KERNEL_KIND=stripped M80_ROOTFS_IMAGE=/tank/tmp/m80-build/post-restore-current/output.ext4 M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker M80_JAILER_BIN=/opt/firecracker/bin/jailer M80_FIRECRACKER_SECCOMP_FILTER=/opt/firecracker/bin/firecracker-seccomp-filter.bin M80_JAILER_HARDEN_BIN=/opt/m80/bin/m80-jailer-harden M80_NET_HELPER_BIN=/opt/m80/bin/m80-net-helper M80_FIRECRACKER_VERSION=v1.15.1 M80_JAIL_UID=1000 M80_JAIL_GID=1000 ./scripts/smoke-pmem-shared.sh`

## Substrate

- host kernel: `6.17.0-23-generic`
- firecracker: `Firecracker v1.15.1; 2026-05-17T11:18:22.305717924 [anonymous-instance:main] Firecracker exiting successfully. exit_code=0`
- `/dev/kvm`: `crw-rw---- root:kvm /dev/kvm`
- sudo: required; test ran as uid `0`
- dropped page cache before each cycle: `sync && echo 3 > /proc/sys/vm/drop_caches`
- quiet host check: `true`
- allow other Firecracker VMs: `false`
- git worktree dirty excluding this artifact: `false`
- git commit: `8c3e8d4cbfba04a74571b6a32b98eff8298def64`

### Firecracker process substrate

```json
{
  "allow_other_firecracker_vms": false,
  "dev_kvm_stat": "crw-rw---- root:kvm /dev/kvm",
  "firecracker_version": "Firecracker v1.15.1\n\n2026-05-17T11:17:18.268975138 [anonymous-instance:main] Firecracker exiting successfully. exit_code=0",
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
    "kernel_image_sha256": "c453f36520d2f2792ab8e4532a814e4a647a4a41a4c94d4e9083a502800159b1",
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
- max observed delta: `214664 KiB`
- result: `pass`

## Teardown

- max active-use markers observed: `4`
- final active-use markers: `0`
- stale markers swept after teardown: `0`
- canonical Shared artifact present after teardown: `true`

## Samples

| cycle | MemAvailable before KiB | MemAvailable after KiB | delta KiB | bound KiB |
|---:|---:|---:|---:|---:|
| 1 | 151405816 | 151233272 | 172544 | 655364 |
| 2 | 151392100 | 151299828 | 92272 | 655364 |
| 3 | 151436916 | 151222252 | 214664 | 655364 |
| 4 | 151414864 | 151276508 | 138356 | 655364 |
| 5 | 151425376 | 151272596 | 152780 | 655364 |
| 6 | 151429992 | 151268408 | 161584 | 655364 |
| 7 | 151391404 | 151237980 | 153424 | 655364 |
| 8 | 151395720 | 151269840 | 125880 | 655364 |
| 9 | 151419848 | 151232944 | 186904 | 655364 |
| 10 | 151414740 | 151280852 | 133888 | 655364 |

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

