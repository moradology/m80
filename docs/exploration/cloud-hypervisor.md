# Cloud-Hypervisor Storage Architecture — Exploration Notes

Findings from reading the cloud-hypervisor source at `main` (~v51, May 2026) and adjacent docs. Relevant to m80's pivot from full-copy base images to shared read-only base + per-VM overlay.

---

## 1. Drive Model

Cloud-hypervisor supports **both** virtio-blk and virtio-fs, plus virtio-pmem. Firecracker supports only virtio-blk.

**virtio-blk** (`virtio-devices/src/block.rs`, `block/src/lib.rs`):
- First-class, always compiled in. Activated by `--disk path=<img>`.
- Formats supported: raw, qcow2, fixed VHD, VHDX (`block/src/lib.rs`, `ImageType` enum, lines ~870–887).
- Async I/O via io_uring when the `io_uring` feature flag is enabled (conditional compile, lines ~585–656).
- `DiskConfig` in `vmm/src/config.rs` (~line 1722) exposes: `readonly: bool`, `direct: bool`, `sparse: bool`, `backing_files: bool`, `image_type: ImageType`, `vhost_user: bool`, `lock_granularity: LockGranularityChoice`.

**virtio-fs / vhost-user-fs** (`virtio-devices/src/` — the `--fs` flag):
- Implemented as vhost-user: a separate `virtiofsd` daemon runs on the host and speaks the vhost-user protocol over a socket. Cloud-hypervisor attaches to the socket; it does not implement the filesystem driver itself.
- DAX window (shared host/guest memory) is spec'd but explicitly disabled: "Given the DAX feature is not stable yet from a daemon standpoint, it is not available in Cloud Hypervisor" (`docs/fs.md`).
- Cache modes: `cache=never` (no host page cache; smaller host footprint) or `cache=always` (uses host page cache; better throughput).
- virtiofs-as-rootfs is documented (`docs/virtiofs-root.md`): share a host directory as the guest's `/`, with `rootfstype=virtiofs root=/dev/root` in the kernel cmdline. No block device required.

**virtio-pmem** (`--pmem` flag):
- Bypasses the guest page cache entirely, reducing guest memory footprint. Useful for large read-only data files mapped directly into guest physical address space. Not a general-purpose writable disk.

---

## 2. Read-Only Base Sharing

Yes — N VMs can attach the same host file as a read-only block device simultaneously. The locking mechanism is intentionally cooperative:

- `try_lock_image()` in `virtio-devices/src/block.rs` (~lines 1069–1093) calls `fcntl::try_acquire_lock()` with `LockType::Read` when `read_only()` is true, `LockType::Write` otherwise.
- POSIX `fcntl` read locks (F_RDLCK) are shared: any number of readers can hold the lock concurrently; a write lock is exclusive. This is the standard POSIX advisory-lock semantics — multiple cloud-hypervisor instances attaching the same file `readonly=on` will all succeed.
- Granularity is configurable: whole-file or byte-range (`lock_granularity_choice`), with byte-range preferred for compatibility with network storage backends.
- The lock is **advisory only**. It prevents other cloud-hypervisor processes from accidentally writing to the same file, but any process that ignores the lock can still write. There is no kernel-level block.

**Cache caveat:** With `direct=off` (the default), the host page cache is shared across all processes mapping the same file. On Linux, multiple read-only mmaps or reads of the same inode share a single page-cache entry — so N VMs reading the same base image pay roughly O(1) host DRAM for the base image pages, not O(N). With `direct=on`, each VM bypasses the cache, which is slower but avoids cache pressure.

---

## 3. Overlay Patterns

**qcow2 backing files** — first-class, but off by default due to a recent critical CVE.

- `block/src/qcow/mod.rs` implements a full backing-file chain: `BackingFile { kind: BackingKind }` where `BackingKind` is `Raw(RawFile)` or `Qcow { inner, backing: Option<Box<BackingFile>> }` (~lines 180–229). Reads of unmapped clusters delegate to the backing file; writes land in the overlay layer only.
- `DiskConfig.backing_files: bool` gates whether the qcow2 parser will follow a backing-file pointer. **Default is `false` since v50.1/v51.0** (February 2026), following CVE GHSA-jmr4-g2hv-mjj6: a malicious guest could rewrite its own disk header to point the backing-file path at any host file, exfiltrating it on next boot. Affected v34.0–v50.0.
- The intended workflow (when `backing_files=on` and images come from trusted sources): create a shared read-only raw or qcow2 base, then per-VM `qemu-img create -f qcow2 -b base.img vm-N.qcow2`. Each VM gets its own small overlay; the base is opened read-only. The per-VM qcow2 is the writable disk.
- RAW backing file support (a qcow2 overlay on top of a raw base) was added before the CVE freeze. This is the most useful pattern for m80: keep the base as an immutable raw ext4, create a per-VM sparse qcow2 overlay in O(1) (just a header write), boot the VM with the overlay as its disk.

**In-guest overlayfs:**
- Nothing in cloud-hypervisor is specific to this. A VM can boot a minimal root, then mount a read-only block device as lower layer and a writable tmpfs/ext4 as upper layer using in-kernel overlayfs. Cloud-hypervisor does not automate this; it is a guest-side concern.

**virtiofs-as-rootfs + writable overlay block device:**
- Documented pattern: share a host directory (the base OS tree) via virtiofsd as the guest root, add a small virtio-blk disk for the writable overlay or scratch space. The host directory can be shared read-only across N virtiofsd instances pointing at the same path.

---

## 4. Performance Numbers

No official cold-boot latency figures are published by the project. From external sources and the issue tracker:

- **VM startup:** ~200 ms cold boot (cloud-hypervisor) vs ~125 ms (Firecracker). The ~75 ms delta is attributed to the broader device model (hotplug infrastructure, wider hardware compatibility) (`northflank.com/blog/guide-to-cloud-hypervisor`).
- **virtio-blk throughput** (NVMe host, issue #4387): random read 88 K IOPS / 2160 MiB/s; random write 87 K IOPS / 1768 MiB/s — close to bare-metal NVMe (332 K / 2690 MiB/s read). The gap narrows significantly with batched async-IO submission (a fix landed in a recent release).
- **virtio-fs throughput** (same benchmark, issue #4387): random read 13.5 K IOPS / 610 MiB/s — roughly 75% degradation vs. baseline. Write is better: 49 K IOPS / 1259 MiB/s.
- **qcow2 overlay attach:** no published numbers. A `qemu-img create -f qcow2 -b base.img overlay.qcow2` is a metadata-only operation (microseconds); the overlay file starts at ~200 KB. First-read latency from the backing file is identical to opening the base directly, minus one extra header parse.
- **No published image-copy elimination latency.** The closest analogy to m80's 727 ms file-copy problem is the qcow2 overlay: the per-VM setup cost drops to the overlay file creation time (sub-millisecond) plus one extra `open()` for the backing file at VM start.

---

## 5. Portability to Firecracker

| Pattern | Cloud-Hypervisor | Firecracker | Notes |
|---|---|---|---|
| `readonly=on` virtio-blk (N VMs, same file) | Yes — shared F_RDLCK | **Yes** — Firecracker opens block devices with standard POSIX file I/O; read-only attach of the same file works. Firecracker has no locking layer of its own, so concurrent read-only access is safe at the kernel level (shared page cache) | Low risk |
| qcow2 overlay + raw backing file | Yes (`backing_files=on`, default off) | **No** — Firecracker's block backend is raw-only. No qcow2 format support at all. | Not portable |
| virtio-fs / virtiofs-as-rootfs | Yes | **No** — Firecracker deliberately omits virtio-fs. Cited reason: filesystem drivers are a large kernel attack surface. | Not portable |
| virtio-pmem | Yes | No | Not portable |
| vhost-user-blk (external backend, e.g. SPDK) | Yes | No | Not portable |
| In-guest overlayfs on two virtio-blk disks | Yes | **Yes** — boot from a read-only base disk (or any read-write disk), mount a second writable disk, configure overlayfs inside the guest. | Fully portable |
| `direct=on` (O_DIRECT, bypass host page cache) | Yes | **Yes** — Firecracker exposes `is_read_only` and the file is opened with `O_RDWR` or `O_RDONLY`; `O_DIRECT` is not directly exposed but the guest driver behavior is the same | Partial |

---

## Patterns Worth Considering for m80

**Immediately portable (virtio-blk only):**

1. **Shared read-only base + per-VM writable scratch disk.** Open the base ext4 read-only (Firecracker supports this). Create a per-VM sparse ext4 or raw file as the writable root. Inside the guest, mount base as a block device and use in-guest overlayfs with a writable upper dir on the second disk. Cost: two `open()` calls at VM start instead of a 256 MiB file copy. The base image pages are shared in the host page cache across all VMs — the 727 ms disappears.

2. **Per-VM sparse copy-on-write via `FALLOC_FL_PUNCH_HOLE`.** On Linux, `cp --reflink=always` on btrfs/XFS creates a CoW clone in O(metadata) time. On ext4 (most common), reflinks are not available; use a sparse file with the base as a pre-read seed instead. Less clean than qcow2 but Firecracker-compatible.

**Not portable, cloud-hypervisor-only:**

3. **qcow2 overlay on raw base.** Elegant — per-VM state is one small overlay file, base is never written. Requires switching from Firecracker to cloud-hypervisor. The security caveat (CVE GHSA-jmr4-g2hv-mjj6) applies only when images come from untrusted sources; m80-controlled images are safe with `backing_files=on`.

4. **virtiofs shared rootfs.** N virtiofsd instances can share the same host directory. Eliminates the block device copy entirely. The 75% read-IOPS penalty vs. virtio-blk is meaningful for write-heavy workloads; for agent-style read-mostly sandbox execution it may be acceptable. Requires switching VMM.

---

## Summary

Cloud-hypervisor solves the shared-base problem two ways: (a) qcow2 overlay on a read-only raw base (block-side CoW, VMM-managed) and (b) virtiofs shared host directory (filesystem passthrough, N VMs point at same host path). Neither transfers to Firecracker. The Firecracker-compatible path is in-guest overlayfs on two virtio-blk disks — base opened read-only (shared host page cache), per-VM writable disk created as a sparse file in O(milliseconds). That eliminates the 256 MiB copy without switching VMM or adding kernel attack surface.
