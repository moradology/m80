# OverlayFS Kernel Semantics: Technical Brief

Target use case: ext4 base image on `/dev/vda` (lower), ext4 per-VM disk on `/dev/vdb` (upper + workdir),
overlayfs mounted at `/`, pivot_root into it. Guest runs Linux 5.10.245 (Firecracker CI kernel).

---

## 1. Mount-Time Invariants

**Mount(2) data string format:**

```
"lowerdir=/mnt/lower,upperdir=/mnt/upper,workdir=/mnt/upper/.work"
```

All three paths are comma-separated in the `data` argument to `mount(2)`. The filesystem type is `"overlay"` and the device string (source) is conventionally `"overlay"` (ignored by the kernel).

**Maximum lowerdirs:** There is no hard-coded upper limit documented in the kernel source. In practice, the limit is constrained by the length of the `data` string passed to `mount(2)` (one `PAGE_SIZE`, 4096 bytes). With one lowerdir this is entirely safe. Multiple lowerdirs use `:` as separator: `lowerdir=/a:/b:/c`.

**workdir requirements:**
- Must be an **empty directory** on the **same filesystem instance** (same superblock) as `upperdir`.
- Cannot be omitted when `upperdir` is specified. Omitting `workdir` with an `upperdir` present causes `EINVAL` at mount time.
- If `workdir` is non-empty at mount time: the kernel does not document defined behavior. Empirically, the kernel will attempt to clean up stale work files on mount but this is not guaranteed. **Treat a non-empty workdir as undefined behavior — always provide a freshly empty directory or a freshly formatted upper filesystem.** For our use case (freshly formatted `/dev/vdb`), the workdir directory must be created after mkfs and before mounting overlay.

**Workdir inaccessibility:** The kernel takes exclusive control of the workdir at mount time. It is not accessible through the merged mount and must not be accessed via the upper filesystem path during overlay lifetime.

---

## 2. Filesystem Requirements

**Upper + workdir:** Must reside on the **same filesystem superblock** — same partition, same mount. "Same fs type" is not sufficient; two separate ext4 mounts do not satisfy this requirement. The upper filesystem must:
- Be writable (not mounted read-only).
- Support `trusted.*` extended attributes (ext4 supports this by default).
- Return valid `d_type` in readdir responses (ext4 does; NFS does not — NFS is explicitly documented as unsuitable for upper).

**Lower:** Can be any Linux-supported filesystem, read-only or read-write, including another overlayfs mount. No xattr support required. No `d_type` requirement for read-only use. ext4 is fully supported as a lower layer.

**Our stack: ext4 (lower, `/dev/vda`) → ext4 (upper+work, `/dev/vdb`) → overlayfs:** This is a well-supported configuration. Both layers are block-backed ext4; there is no loop device involved. The two ext4 instances are on different superblocks, which is fine — the same-superblock requirement only applies between upper and workdir (which are on `/dev/vdb`).

---

## 3. Mount Sequence: Block Devices Must Be Mounted First

OverlayFS `lowerdir`, `upperdir`, and `workdir` arguments are **directory paths in the VFS namespace**. They must point to directories within already-mounted filesystems. You cannot pass a block device path like `/dev/vda` directly.

**Required sequence:**

1. Mount the lower block device to a directory: `mount("/dev/vda", "/mnt/lower", "ext4", MS_RDONLY, NULL)`.
2. Mount the upper block device to a directory: `mount("/dev/vdb", "/mnt/upper", "ext4", 0, NULL)`.
3. Create the workdir if it doesn't exist: `mkdir("/mnt/upper/.work", 0700)`.
4. Mount overlayfs: `mount("overlay", "/mnt/merged", "overlay", 0, "lowerdir=/mnt/lower,upperdir=/mnt/upper,workdir=/mnt/upper/.work")`.
5. Pivot root into `/mnt/merged`.

The workdir path (`/mnt/upper/.work`) is on `/dev/vdb` — same superblock as upperdir (`/mnt/upper`). This satisfies the constraint.

---

## 4. Whiteouts and Deletions

From the kernel documentation (verbatim):

> "In order to support rm and rmdir without changing the lower filesystem, an overlay filesystem needs to record in the upper filesystem that files have been removed. This is done using whiteouts and opaque directories. A whiteout is created as a character device with 0/0 device number. When a whiteout is found in the upper level of a merged directory, any matching name in the lower level is ignored, and the whiteout itself is also hidden."

When the guest `unlink`s `/etc/hosts` (which exists only in lower), overlayfs creates a character device node with major/minor `0:0` at `/upper/etc/hosts`. The merged view shows the file as deleted. The lower layer is not modified.

Directory removal: a directory in lower is hidden by setting `trusted.overlay.opaque=y` on the upper copy.

An alternative whiteout form introduced for nested overlays uses a zero-size regular file with `trusted.overlay.whiteout` xattr. Both forms are transparent to overlayfs consumers.

---

## 5. Mount Options: redirect_dir, metacopy, nfs_export, index, xino

**redirect_dir** (default: off unless `CONFIG_OVERLAY_FS_REDIRECT_DIR=y` in kernel build):
- Controls handling of directory renames that cross layers. Without it, renaming a directory from the lower/merged side returns `EXDEV`.
- For our use case (VM sandbox, not a container runtime doing package installs): **leave off**. Directory renames stay within upper after copy-up; `EXDEV` is unlikely to matter in practice. If guest userspace hits `EXDEV` on directory rename, set `redirect_dir=on`.

**metacopy** (default: off unless `CONFIG_OVERLAY_FS_METACOPY=y`):
- When on, only metadata (owner, permissions, timestamps) is copied up on `chown`/`chmod`; file data stays in lower until write-open. Reduces copy-up I/O for workloads that chown many files.
- **Security warning from kernel docs:** "Do not use metacopy=on with untrusted upper/lower directories" — attackers can craft REDIRECT and METACOPY xattrs on the upper fs to redirect data reads to arbitrary lower files.
- For our use case: both upper and lower are controlled by m80. **Safe to enable if desired**, but the gain is minimal for a general-purpose sandbox that will write to most files anyway. **Leave off** unless profiling shows copy-up is a bottleneck.

**nfs_export** (default: off):
- Enables stable file handles for NFS re-export of the overlay mount.
- **Not needed.** Leave off. Enabling it with a read-write mount conflicts with `index=off` and produces `EINVAL`.

**index** (default: off unless `CONFIG_OVERLAY_FS_INDEX=y`):
- Preserves hard-link relationships across copy-up. Without it, copying up a file with multiple hard links breaks the links.
- Uses `trusted.overlay.origin` xattr on upper root on first mount.
- **Not needed** for a single-boot sandbox where hard-link identity in upper doesn't matter. Leave off.

**xino** (default: auto with `CONFIG_OVERLAY_FS_XINO_AUTO=y`, otherwise off):
- Composes a unique `st_ino` by embedding the underlying fsid into high inode bits. Ensures `st_ino` is stable and unique across layers.
- When all layers share one underlying filesystem: automatic and free. When they don't (our case — two different ext4 superblocks): uses high inode bits as fsid index.
- **Recommendation:** Accept the default (`xino=auto`). It gives better POSIX `stat` behavior with no downside for our use case. If the kernel's `CONFIG_OVERLAY_FS_XINO_AUTO` is not set in the firecracker kernel, explicitly pass `xino=on`.

---

## 6. Kernel Configuration Requirements

**Required:**
- `CONFIG_OVERLAY_FS=y` — base overlayfs support. Must be built-in (`=y`), not a module (`=m`), if it needs to be available before module loading infrastructure is up (e.g., as PID 1 before init).

**Optional feature knobs (compile-time defaults for the corresponding mount options):**
- `CONFIG_OVERLAY_FS_REDIRECT_DIR` — sets redirect_dir default on.
- `CONFIG_OVERLAY_FS_REDIRECT_ALWAYS_FOLLOW` — always follow redirect xattrs (security-relevant).
- `CONFIG_OVERLAY_FS_INDEX` — sets index default on.
- `CONFIG_OVERLAY_FS_METACOPY` — sets metacopy default on.
- `CONFIG_OVERLAY_FS_XINO_AUTO` — sets xino=auto as default.

**Xattr support on upper filesystem:** ext4 has `CONFIG_EXT4_FS_XATTR` (on by default in all distro kernels). No extra config needed.

**d_type in ext4:** Always present; no config knob needed.

For the Firecracker CI kernel (5.10.245), verify the kernel `.config` has `CONFIG_OVERLAY_FS=y`. The Firecracker team's guest kernel configs historically include it.

---

## 7. Kernel Version History

- **3.18 (2014):** OverlayFS merged into mainline. Supported upper+lower with trusted xattr whiteouts. `workdir` was present from the initial upstream merge (it was a pre-merge requirement to fix atomicity).
- **4.0 (2015):** Multiple lower layers (`:` separator).
- **4.19 (2018):** `metacopy` (metadata-only copy-up). Stack file operations enabling POSIX compliance.
- **5.10 (2020):** `volatile` mount option. ioctl support. Long-Term Support release.
- **5.x+:** `xino=auto`, `uuid`, various index improvements.

**Linux 5.10.245:** All features relevant to our use case (workdir, whiteouts, multiple lowerdirs, xino, metacopy) are present and stable. No missing feature concerns.

---

## 8. Failure Modes During Boot

If the overlayfs `mount(2)` call fails:
- The syscall returns `-1` with `errno` set (`EINVAL` for bad options, `ENOENT` if a path doesn't exist, `EPERM` if missing xattr capability, `ENOMEM` under memory pressure).
- The overlay is not mounted. No partial state is left in the VFS.
- The underlying mounts (`/mnt/lower`, `/mnt/upper`) remain mounted and intact.
- The workdir directory may have been partially modified (the kernel pre-cleans it). Since we freshly formatted `/dev/vdb`, this is not a concern.

**Cleanup on failure:** If overlayfs mount fails, m80-guestd should:
1. Log the errno.
2. Unmount `/mnt/upper` and `/mnt/lower` with `umount2(MNT_DETACH)` to release the block devices cleanly.
3. Panic or exit with a non-zero status — there is no safe fallback to running on the bare lower layer.

There is no risk of corrupting the lower (`/dev/vda`) on overlayfs mount failure; lower is mounted read-only and overlayfs never writes to it.

---

## 9. pivot_root + OverlayFS Interaction

`pivot_root(2)` requires:
- `new_root` must be a **mount point** (not just a directory). The overlayfs mount at `/mnt/merged` satisfies this — it is a mount point by definition.
- `new_root` must not be on the **same mount** as the current root.
- `put_old` must be **at or under** `new_root`.
- The **propagation type** of the parent mount of `new_root` and the parent mount of the current root must not be `MS_SHARED`.

**Required flag before pivot_root:** Make the entire mount tree private to prevent propagation to the host:

```c
mount(NULL, "/", NULL, MS_REC | MS_PRIVATE, NULL);
```

This is necessary because in a new mount namespace (which Firecracker provides), the root mount may still be `MS_SHARED` by propagation inheritance. Setting `MS_REC | MS_PRIVATE` on `/` before proceeding is safe and required.

OverlayFS mounts are fully compatible as `new_root` for `pivot_root`. The kernel imposes no filesystem-type restriction on `new_root`.

---

## Minimum Mount Call Sequence (Pseudocode)

```c
// 1. Isolate the mount namespace from propagation
mount(NULL, "/", NULL, MS_REC | MS_PRIVATE, NULL);   // must not fail

// 2. Mount the read-only base image (lower layer)
mkdir("/mnt/lower", 0755);
mount("/dev/vda", "/mnt/lower", "ext4", MS_RDONLY, NULL);

// 3. Mount the per-VM writable disk (upper layer + workdir)
mkdir("/mnt/upper", 0755);
mount("/dev/vdb", "/mnt/upper", "ext4", 0, NULL);

// 4. Create workdir on the upper filesystem (must be empty)
mkdir("/mnt/upper/.work", 0700);

// 5. Create the merge target
mkdir("/mnt/merged", 0755);

// 6. Mount overlayfs
const char *opts =
    "lowerdir=/mnt/lower,upperdir=/mnt/upper,workdir=/mnt/upper/.work";
mount("overlay", "/mnt/merged", "overlay", 0, opts);
// On failure: umount2 upper and lower, then panic.

// 7. Prepare put_old inside the new root
mkdir("/mnt/merged/old_root", 0700);

// 8. Pivot
pivot_root("/mnt/merged", "/mnt/merged/old_root");

// 9. Fix up cwd (pivot_root does not update it)
chdir("/");

// 10. Unmount and remove old root
umount2("/old_root", MNT_DETACH);
rmdir("/old_root");   // optional; cleans up the merged view

// 11. Continue: exec init or guestd's main loop
```

**Notes on the sequence:**
- Step 1 (`MS_REC | MS_PRIVATE`) must come before step 8. Without it, `pivot_root` will return `EINVAL` if the root mount propagation type is `MS_SHARED`.
- `/dev/vda` is mounted `MS_RDONLY` — overlayfs never needs to write to lower, and this prevents accidental corruption.
- The workdir path (`/mnt/upper/.work`) is on `/dev/vdb` — same superblock as `upperdir` (`/mnt/upper`). This satisfies the kernel requirement.
- No `MS_BIND` self-bind is needed on `/mnt/merged` because it is already a distinct mount (the overlay mount), not a plain directory.
- After step 10, `/dev/vda` and `/dev/vdb` remain accessible through the overlayfs; only the staging mount points in the old root are gone.
