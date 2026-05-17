# Pmem Erofs DAX Kernel Support

Bead: `m80-q420k.2.15`

The stripped m80 guest kernel profile must keep the whole pmem-backed erofs
stack built in. Phase B layered rootfs work depends on PID 1 being able to
mount a Firecracker virtio-pmem device as read-only erofs with DAX before any
guest module-loading path exists.

The required built-in config surface is:

- erofs: `CONFIG_EROFS_FS=y`, `CONFIG_EROFS_FS_ZIP=y`,
  `CONFIG_EROFS_FS_ZIP_LZ4=y`, `CONFIG_EROFS_FS_ZIP_LZ4HC=y`
- virtio-pmem and libnvdimm: `CONFIG_VIRTIO_PMEM=y`,
  `CONFIG_LIBNVDIMM=y`, `CONFIG_BLK_DEV_PMEM=y`
- filesystem DAX: `CONFIG_FS_DAX=y`, `CONFIG_DAX=y`,
  `CONFIG_NVDIMM_PFN=y`, `CONFIG_NVDIMM_DAX=y`
- memory model dependencies that keep FS DAX enabled through `olddefconfig`:
  `CONFIG_SPARSEMEM_MANUAL=y`, `CONFIG_SPARSEMEM=y`,
  `CONFIG_SPARSEMEM_VMEMMAP=y`, `CONFIG_MEMORY_HOTPLUG=y`,
  `CONFIG_MEMORY_HOTREMOVE=y`, `CONFIG_ZONE_DEVICE=y`

`m80-image-store` mirrors that kernel floor at import time. Erofs artifacts are
admitted only when `dump.erofs -s` reports the pinned feature/compressor set:
`sb_csum`, `mtime`, `0padding`, and LZ4/LZ4HC compression. Newer host
`mkfs.erofs` outputs using zstd, lzma, deflate, chunked files, or other feature
bits are rejected before content-addressing so the mismatch cannot surface as a
guest boot-time mount failure.

The real-KVM smoke on 2026-05-16 rebuilt the stripped kernel as:

```text
crates/m80-image-build/kernels/vmlinux-m80-613988fdb6a6aaa0f806ec27e6f6e66875e2f768b28a2c0244aa4af3d8e2ac19.bin
```

Using Firecracker v1.15.1, a disposable Ubuntu rootfs, and
`/tank/tmp/m80-q420k-poc/rust-toolchain-1.82.erofs` as the pmem backing file,
the guest observed `/dev/pmem0`, reported `pmem0/queue/dax=1`, and mounted:

```text
/dev/pmem0 /opt/toolchain erofs ro,relatime,user_xattr,acl,cache_strategy=readaround,dax=always 0 0
```

`/opt/toolchain/bin/cargo --version` and
`/opt/toolchain/bin/rustc --version` both exited 0 from that mount. A follow-up
snapshot/restore smoke using the same kernel and erofs pmem image also
returned `204` for pause, snapshot, and load; after restore the guest still had
the erofs DAX mount and `cargo` exited 0 without remounting.

Evidence:

- `docs/poc/2026-05-16-layered-rootfs-poc-findings.md`
- `/tank/tmp/m80-q420k-poc/fc-pmem-erofs-rebuilt-boot-verified/probe-summary.txt`
- `/tank/tmp/m80-q420k-poc/fc-pmem-erofs-snapshot-4/poc3-poc4-erofs-run.json`

Tests:

- `crates/m80-image-build/tests/kernel_build_pipeline.rs::stripped_config_keeps_virtio_pmem_and_dax_built_in`
- `crates/m80-image-build/tests/kernel_build_pipeline.rs::stripped_config_keeps_zone_device_memory_model_for_fs_dax`

Limitations:

- The smoke proves the kernel exposes pmem, accepts erofs with `dax=always`,
  and executes binaries from the mounted image. It does not prove host page
  cache sharing or file-level DAX behavior across multiple guests.
- The erofs image used in the PoC was compressed. The later
  `docs/perf/erofs-dax-sharing-layout.md` gate proved that compressed erofs
  files do not get guest-visible `STATX_ATTR_DAX`; Phase C/F density proofs
  must use uncompressed non-inlined erofs payload files.
- Firecracker logs show VMGenID changes on restore, but this guest kernel/rootfs
  did not expose a userspace `vmgenid` sysfs counter.
