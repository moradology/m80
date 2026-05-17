# Erofs DAX sharing layout

Bead: `m80-q420k.8.12`

## Result

Only uncompressed non-inlined erofs files are eligible for file-level DAX on this substrate. Compressed erofs files still mount on the pmem device with `dax=always`, but `statx` inside the guest does not report `STATX_ATTR_DAX` for the file.
Therefore Phase C/F Shared-pmem density measurements must use an uncompressed erofs payload layout. `m80-firecracker` Shared admission now rejects erofs artifacts whose `dump.erofs -S` output reports compressed files before the measurement path can launch.

## Reproduction

Command: `sudo -n env M80_EROFS_DAX_LAYOUT_ARTIFACT=/tank/projects/m80/docs/perf/erofs-dax-sharing-layout.md M80_RUN_ROOT=/var/lib/m80-edl M80_ROOTFS_IMAGE=/tank/tmp/m80-build/shared-pmem-ubuntu2/output.ext4 M80_KERNEL_IMAGE=/tank/projects/m80/crates/m80-image-build/kernels/vmlinux-m80-613988fdb6a6aaa0f806ec27e6f6e66875e2f768b28a2c0244aa4af3d8e2ac19.bin M80_KERNEL_KIND=stripped M80_JAIL_UID=<uid> M80_JAIL_GID=<gid> target/debug/deps/erofs_dax_layout_real_kvm-<hash> erofs_dax_layout_matrix_real_kvm --ignored --exact --nocapture`

## Substrate

- host kernel: `6.17.0-23-generic`
- firecracker: `Firecracker v1.15.1`
- erofs-utils: `mkfs.erofs (erofs-utils) 1.8.10; available compressors: lz4, lz4hc, lzma, deflate, libdeflate, zstd`
- kernel/userland rule: local `erofs(5)` documents that `dax=always` direct reads apply to uncompressed non-inlined files.

## Matrix

| case | mkfs shape | image bytes | statx size | dump layout | statx dax | statx compressed | statx mask | statx attrs | mount |
|---|---|---:|---:|---|---:|---:|---|---|---|
| `flat-uncompressed` | `mkfs.erofs without -z` | 34430976 | 33554432 | ` 40   Links: 1   Layout: 0   Compression ratio: 100.00%` | true | false | `0x17df` | `0x200010` | `/dev/pmem0 /opt/m80-layers/smoke-0 erofs ro,nosuid,nodev,relatime,user_xattr,acl,cache_strategy=readaround,dax=always 0 0` |
| `flat-compressed-lz4hc` | `mkfs.erofs -zlz4hc,level=9` | 675840 | 33554432 | ` 20480   Links: 1   Layout: 3   Compression ratio: 0.42%` | false | true | `0x17df` | `0x14` | `/dev/pmem0 /opt/m80-layers/smoke-0 erofs ro,nosuid,nodev,relatime,user_xattr,acl,cache_strategy=readaround,dax=always 0 0` |
| `toolchain-proxy-compressed-lz4hc` | `mkfs.erofs -zlz4hc,level=9 with bin/lib/include tree` | 675840 | 33554432 | ` 20480   Links: 1   Layout: 3   Compression ratio: 0.42%` | false | true | `0x17df` | `0x14` | `/dev/pmem0 /opt/m80-layers/smoke-0 erofs ro,nosuid,nodev,relatime,user_xattr,acl,cache_strategy=readaround,dax=always 0 0` |

## Raw dump.erofs output

### flat-uncompressed

```text
Path : /payload.bin
Size: 33554432  On-disk size: 33554432  regular file
NID: 40   Links: 1   Layout: 0   Compression ratio: 100.00%
Inode size: 32   Xattr size: 0
Uid: 0   Gid: 0  Access: 0644/rw-r--r--
Timestamp: 1970-01-01 00:00:00.000000000
```

### flat-compressed-lz4hc

```text
Path : /payload.bin
Size: 33554432  On-disk size: 139264  regular file
NID: 20480   Links: 1   Layout: 3   Compression ratio: 0.42%
Inode size: 32   Xattr size: 0
Uid: 0   Gid: 0  Access: 0644/rw-r--r--
Timestamp: 1970-01-01 00:00:00.000000000
```

### toolchain-proxy-compressed-lz4hc

```text
Path : /lib/libpayload.a
Size: 33554432  On-disk size: 139264  regular file
NID: 20480   Links: 1   Layout: 3   Compression ratio: 0.42%
Inode size: 32   Xattr size: 0
Uid: 0   Gid: 0  Access: 0644/rw-r--r--
Timestamp: 1970-01-01 00:00:00.000000000
```
