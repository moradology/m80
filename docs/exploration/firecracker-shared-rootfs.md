# Firecracker shared RO base-disk: source-level findings

**Date:** 2026-05-04  
**Firecracker source:** `/tank/projects/firecracker` (current main branch)  
**Purpose:** Determine whether attaching the same host file read-only from N concurrent
Firecracker processes is safe and whether host page-cache deduplication applies.

---

## 1. `is_read_only` semantics: REST → virtio-blk

The REST drive-PUT field `is_read_only` (type `Option<bool>`) is defined in
`src/vmm/src/vmm_config/drive.rs:53`. It flows through two transformations before
the file is opened:

1. **Config→internal**: `BlockDeviceConfig` is converted to `VirtioBlockConfig` in
   `src/vmm/src/devices/virtio/block/virtio/device.rs:202-221`. The field defaults
   to `false` if absent (`is_read_only.unwrap_or(false)`, line 213).

2. **File open**: `DiskProperties::open_file` (same file, lines 65-71) does:
   ```rust
   OpenOptions::new()
       .read(true)
       .write(!is_disk_read_only)
       .open(PathBuf::from(&disk_image_path))
   ```
   When `is_read_only=true`, `.write(false)` is passed, so the kernel opens the
   file `O_RDONLY`. No `O_CREAT`, no `O_TRUNC`, no `O_DIRECT`.

3. **Virtio feature flag**: In `VirtioBlock::new` (lines 306-308) `VIRTIO_BLK_F_RO`
   is set in `avail_features` when the drive is read-only. The guest driver sees this
   and refuses to issue write requests to that device. The host side never enforces a
   separate write-check; the POSIX permission (`O_RDONLY`) would cause a kernel EBADF
   if a write somehow reached the engine, but that path is unreachable in practice
   because the guest driver obeys `VIRTIO_BLK_F_RO`.

**No file-level advisory or mandatory locks (`flock`/`fcntl`) are acquired** at any
point in the drive-attach path. A grep across all `*.rs` source files for `flock`,
`LOCK_SH`, `LOCK_EX`, `advisory_lock` returns zero hits in vmm or device code. The
host OS does not prevent N processes from opening the same file `O_RDONLY`
simultaneously—that is standard POSIX read-sharing semantics.

---

## 2. Drive-PUT order → /dev/vdN naming

Guest device naming is determined by the order blocks are registered with the MMIO
device manager. The invariant is documented explicitly in
`src/vmm/src/device_manager/mmio.rs:135-141`:

> "We create the AML byte code for every VirtIO device in the order we build it,
> so that we ensure the root block device appears first in the DSDT. This is needed,
> so that the root device appears as `/dev/vda` in the guest filesystem."

`BlockBuilder::insert` (`src/vmm/src/vmm_config/drive.rs:131-178`) enforces that
the drive with `is_root_device=true` is always at the front of the `VecDeque`
regardless of PUT order. `attach_block_devices` in `src/vmm/src/builder.rs:673-706`
iterates `block.devices.iter()` in VecDeque order, emitting one MMIO slot per
device. The root device gets the first MMIO slot → `/dev/vda`; subsequent non-root
drives get `/dev/vdb`, `/dev/vdc`, etc., in their PUT order.

This ordering has been stable since the MMIO transport was introduced and is
structurally coupled to the ACPI DSDT generator; it is not a coincidence or a
convention—it is the designed contract.

---

## 3. I/O engine and host page-cache

Firecracker offers two I/O engines, selectable per drive via `io_engine`:

**Sync (default):** `SyncFileEngine`
(`src/vmm/src/devices/virtio/block/virtio/io/sync_io.rs`).
Reads are issued via `File::read_exact_volatile` (standard `read(2)` syscall) after
a `seek`. Writes via `File::write_all_volatile`. There is **no `O_DIRECT`** flag
anywhere in the file-open path. The file is opened with plain `OpenOptions`, which
defaults to buffered kernel I/O. All reads therefore flow through the Linux page
cache. Multiple processes opening the same inode read-only share the same page-cache
pages—the kernel's page cache is keyed on `(inode, page-offset)`, not on the
file-descriptor owner.

**Async (io_uring):** `AsyncFileEngine`
(`src/vmm/src/devices/virtio/block/virtio/io/async_io.rs`). The same `File` handle,
opened identically (no `O_DIRECT`), is registered as a fixed-fd in the io_uring
instance (`IoUring::new` registers `vec![file]` via `IORING_REGISTER_FILES`). By
default, io_uring without `O_DIRECT` also goes through the page cache. The io_uring
setup flags in `src/vmm/src/io_uring/mod.rs:114` use `IORING_SETUP_R_DISABLED`
(security gating) but no `IORING_SETUP_IOPOLL` or `IORING_SETUP_SQPOLL` that would
imply direct I/O.

**Conclusion on caching:** Both engines use buffered I/O. N Firecracker processes
reading the same RO base image share host page-cache pages. There is no per-process
redundant RAM for the disk image data.

---

## 4. Firecracker docs: relevant guidance

`docs/snapshotting/snapshot-support.md:77`:
> "The design **allows sharing of memory pages and read only disks between multiple
> microVMs**."

This is the only explicit statement in the docs acknowledging shared RO disks. The
context is snapshot resume (MAP_PRIVATE memory sharing), but it explicitly includes
"read only disks"—confirming this pattern is intentional, not incidental.

`docs/rootfs-and-kernel-setup.md` covers rootfs image construction but says nothing
about sharing strategies or overlays.

`docs/pmem.md:154-158` explicitly cautions **against** sharing the same backing file
for `virtio-pmem` across VMs due to side-channel risk from shared physical pages.
This warning is **specific to pmem** (which uses `mmap`/`MAP_SHARED`-style access);
it does not apply to virtio-blk, which uses file I/O through the page cache without
exposing shared physical pages to the guest.

No documentation discusses overlayfs inside the guest or host-side COW layers.

---

## 5. Integration tests exercising RO disks

`tests/integration_tests/functional/test_drive_virtio.py:167` attaches a
single read-only scratch drive to one VM and asserts `blockdev` sees it as `ro`.
No test spins up multiple VMs sharing the same backing file.

`tests/framework/microvm.py:1309-1313` shows the test framework's factory logic:
```python
# copy only iff not a read-only rootfs
rootfs_path = rootfs
if rootfs_path.suffix != ".squashfs":
    rootfs_path = Path(vm.path) / rootfs.name
    shutil.copyfile(rootfs, rootfs_path)
```
Read-only rootfs images (`.squashfs`) are **not copied**—the same file path is
passed directly to each microVM, confirming that the Firecracker team runs tests
where N VMs share the same squashfs-backed file. This is the clearest evidence of
deliberate shared-RO-disk usage in the codebase.

`tests/integration_tests/performance/test_memory_overhead.py` spins up 5
microVMs in a loop via `microvm_factory.build(kernel, rootfs, ...)` using the same
`rootfs` object—again sharing the same RO disk across concurrent instances.

---

## 6. TODOs / FIXMEs near drive-handling code

Only one TODO near block-device code:
`src/vmm/src/devices/virtio/block/virtio/event_handler.rs:74`:
```rust
// TODO: also check for errors. Pending high level discussions on how we want
// to handle errors in devices.
```
This is about error propagation in the event loop, unrelated to shared-disk safety.

No FIXMEs, no locking-related TODOs, no known open issues about shared RO disk
correctness in the GitHub issue tracker.

---

## Summary table

| Question | Finding |
|---|---|
| What does `is_read_only: true` do? | Opens host file `O_RDONLY`; sets `VIRTIO_BLK_F_RO` for guest driver |
| Host-level write prevention | Kernel POSIX permissions (`O_RDONLY`); no Firecracker-level check needed |
| File locking on attach? | **None.** No `flock`/`fcntl` anywhere in the drive path |
| N processes, same file? | Allowed by POSIX; no Firecracker-side obstacle |
| I/O path | Buffered (default); no `O_DIRECT` in either Sync or Async engine |
| Page-cache sharing | Yes — shared pages via kernel page cache across all readers of same inode |
| Drive → guest device name | Root device always `/dev/vda`; non-root in PUT order (`vdb`, `vdc`, …) |
| Official doc guidance | "allows sharing … read only disks between multiple microVMs" (snapshot doc) |
| Tests with shared RO file | squashfs rootfs passed to N VMs without copy (framework + perf tests) |
| Open issues / limitations | None found relevant to shared RO virtio-blk |

---

## Verdict: VERIFIED SAFE

Attaching the same host file as a read-only virtio-blk drive from N concurrent
Firecracker processes is **safe and explicitly supported**:

- POSIX `O_RDONLY` shared-opens on the same inode have been safe since Unix V7.
- Firecracker acquires no locks that would serialize or block concurrent opens.
- Buffered I/O means the Linux page cache naturally deduplicates read pages across
  all N processes—there is no per-VM copy of the rootfs data in host RAM.
- The Firecracker snapshot documentation explicitly names "sharing … read only disks
  between multiple microVMs" as a designed feature.
- The test framework already does exactly this for squashfs rootfs images.

The m80 pivot to shared RO base + per-VM sparse overlay is mechanically sound on the
Firecracker side. The only work remaining is m80-side: provisioning the per-VM
overlay (e.g., a sparse ext4 writable drive presented as `/dev/vdb`) and configuring
the guest init to mount an overlayfs with the RO base (`/dev/vda`) as lower dir and
the per-VM drive (`/dev/vdb`) as upper dir.
