# Storage overlay design

**Bead:** `m80-ovrl.1` (DESIGN — lock storage API, drive layout, in-guest mount sequence)  
**Date:** 2026-05-04  
**Status:** normative — all downstream IMPL leaves consume this doc as contract  
**Cross-references:**
- `docs/exploration/firecracker-shared-rootfs.md` — `is_read_only` semantics, drive-PUT → `/dev/vdX` order, page-cache sharing
- `docs/exploration/overlayfs-kernel-semantics.md` — mount-time invariants, failure modes, minimum call sequence
- `docs/exploration/kata-containers.md` — `pivot_rootfs` verbatim source (mount.rs:523-559), attribution
- `docs/exploration/runc-crun-overlayfs-init.md` — overlayfs init sequence cross-reference
- `docs/planning/storage-pivot-bead-plan.md` — full leaf descriptions for `m80-ovrl.*`
- `docs/planning/perf-roadmap-extended.md §1` — risk register, savings confidence, cross-epic deps

---

## 1. m80-storage public surface (final form)

The following is the normative API. IMPL leaf `m80-ovrl.2` produces code that matches this signature set exactly.

```rust
/// Produce the per-VM overlay ext4 and return a `Rootfs` pointing at the
/// shared base and the new overlay.
///
/// Ensures a run-root-local empty ext4 overlay template exists, then clones it
/// to `overlay_dest` through the caller-selected clone mode. `ByteCopy` runs
/// `cp --reflink=never --sparse=auto`; `Reflink` runs
/// `cp --reflink=always --sparse=auto`; `Auto` resolves to one of those two
/// concrete modes before cloning. The base is NOT copied.
///
/// Caller is responsible for sha256 verification of `base` via
/// `m80_image_manifest::Manifest::verify()` before calling `prepare`.
/// This function does not re-verify.
///
/// `overlay_dest`'s parent directory must already exist.  `prepare` does NOT
/// create parent directories — a missing parent returns a typed storage error.
/// (CLAUDE.md: "no silent recovery")
pub fn Rootfs::prepare(
    base: &Path,
    overlay_dest: &Path,
    overlay_size_bytes: u64,
    clone_mode: OverlayTemplateCloneMode,
) -> Result<Rootfs, StorageError>;

/// Wrap an existing (base, overlay) pair without I/O.
/// For tests and recovery scenarios.
pub fn Rootfs::new_at(base: &Path, overlay: &Path) -> Rootfs;

/// The shared, read-only base rootfs.  Same host file across all VMs from
/// this image; host page cache deduplicates. The manifest's `rootfs_format`
/// declares whether it is ext4 or erofs.
pub fn Rootfs::base_path(&self) -> &Path;

/// The per-VM writable overlay ext4 produced by `prepare`.
pub fn Rootfs::overlay_path(&self) -> &Path;
```

**Deleted from v0.1:**
- `Rootfs::clone(base, dest)` — performed a 727 ms byte-for-byte copy. Removed.
- `Rootfs::path()` — single-path model does not survive the base/overlay split. Removed.

**Deleted error variants:**
- `StorageError::BaseSha256Mismatch` — base verification is the caller's responsibility.
- `StorageError::CopyRootfs` — there is no copy operation.

**New error variants:**
- `StorageError::MkfsFailed` — `mkfs.ext4` non-zero exit for rootfs overlay templates.
- `StorageError::OverlayTemplateCreateFailed` — template or lock file creation failed.
- `StorageError::OverlayTemplateMismatch` — existing template metadata/size does not match the requested shape.
- `StorageError::OverlayTemplateCloneFailed` — reflink/plain-copy clone to the per-VM overlay failed.

**Retained unchanged:** `Scratch::create`, `Scratch::extract`, `Scratch::path`, `ChangeSet`, `Rejection`, `RejectionReason`, remaining `StorageError` variants.

**Sparse overlay sizing:** Default 512 MiB. Configurable via `SandboxConfig::overlay_size_bytes`. The empty ext4 template is keyed by schema version and size under the run root, formatted once with `mkfs.ext4 -F`, then trimmed with `fallocate -d` so explicit byte-copy copies only live ext4 metadata. A stale/wrong-size template is a hard error, not silently reused. The per-VM overlay is cloned from that template and grows as the guest writes.

---

## 2. Drive layout and PUT order

Firecracker assigns `/dev/vdN` names in drive-PUT order, with the root device (`is_root_device: true`) unconditionally placed first regardless of PUT sequence (see `firecracker-shared-rootfs.md §2` — `BlockBuilder::insert` enforces front-of-VecDeque for root; `attach_block_devices` iterates in VecDeque order for MMIO slot assignment). The same order feeds Firecracker's virtio-mmio discovery data, whether the guest consumes the generated ACPI DSDT or the legacy `virtio_mmio.device=...` cmdline entries. This is designed-in, not incidental.

| Position | `drive_id`         | Host file                                    | `is_read_only` | `is_root_device` | Guest path   | Purpose |
|---------:|--------------------|----------------------------------------------|:--------------:|:----------------:|:------------:|---------|
| 1        | `rootfs`           | `<image>/output.ext4` or `output.erofs` (shared base) | **true** | **true** | `/dev/vda` | Read-only base rootfs. Same host file across all VMs from this image. Bind-mounted into jailer chroot. Host page cache deduplicates. Filesystem is declared by manifest `rootfs_format`. |
| 2        | `rootfs_overlay`   | `<run_dir>/rootfs.overlay.ext4`              | false          | false            | `/dev/vdb`   | Per-VM sparse ext4. m80-guestd's PID-1 mounts this as the overlayfs upper layer. Grows with guest writes. |
| 3        | `workspace`        | `<run_dir>/scratch.ext4` *(when requested)*  | false          | false            | `/dev/vdc`   | Per-VM workspace ext4. Mounted at `/workspace` inside the pivoted root. Only present when `SandboxConfig::workspace_dir.is_some()`. |

**PUT sequence is load-bearing.** m80-firecracker MUST PUT drives in the order above. `is_read_only: true` must be set explicitly on `rootfs`; there is no acceptable default (the Firecracker REST default is `false`, `firecracker-shared-rootfs.md §1`).

**Workspace target update:** Before this design the workspace was `/dev/vdb`. After the overlay pivot it is `/dev/vdc`. The mount must occur _inside the pivoted root_, after `pivot_root`, not before. See `m80-ovrl.4a` for the corresponding update to the PID-1 mount step.

**`/workspace` directory availability:** `/workspace` must exist in the merged (overlayfs) view at the time the workspace mount is attempted. It is created at image-build time on the base rootfs and surfaces in the overlayfs merged view via the lowerdir — no post-pivot creation is needed.

---

## 3. In-guest sequence (PID 1)

### 3.1 Normative 11-step pseudocode

The following block is the authoritative pseudocode for `m80-ovrl.4`. IMPL leaves paste this block and translate it to Rust + `nix` calls verbatim. The step numbering and ordering are non-negotiable; deviations require a design amendment.

```rust
// ── Phase 1: namespace isolation ──────────────────────────────────────────
// Step 1. Make the current mount namespace fully private so pivot_root(2)
//         does not propagate to the host.  Must come before any pivot_root.
//         Without this, pivot_root returns EINVAL if root propagation is
//         MS_SHARED (which it may be after Firecracker boots the kernel).
mount(None, "/", None, MS_REC | MS_PRIVATE, None)?;

// ── Phase 2: mount layer disks ────────────────────────────────────────────
// Step 2. Verify the image-built /lower mountpoint exists, read
//         m80.rootfs=<ext4|erofs>, then mount the shared read-only base
//         (vda) with the declared filesystem. The initial root is already
//         read-only, so PID 1 must not create this at runtime.
ensure_precreated_mountpoint("/lower")?;
mount("/dev/vda", "/lower", declared_rootfs_format, MS_RDONLY, None)?;

// Step 3. Verify the image-built /upper mountpoint exists, then mount the
//         per-VM writable ext4 (vdb) there.
ensure_precreated_mountpoint("/upper")?;
mount("/dev/vdb", "/upper", "ext4", MsFlags::empty(), None)?;

// ── Phase 3: prepare overlay dirs ─────────────────────────────────────────
// Step 4. Create upperdir and workdir on /dev/vdb's superblock (required
//         by the kernel: upper and workdir must share one superblock).
//         workdir must be empty at mount time.
fs::create_dir_all("/upper/root")?;
fs::create_dir_all("/upper/.work")?;     // must be empty (freshly mkfs'd)

// Step 5. Verify the image-built /merged mountpoint exists. This is only a
//         mount target; it disappears after pivot.
ensure_precreated_mountpoint("/merged")?;

// ── Phase 4: overlayfs ────────────────────────────────────────────────────
// Step 6. Mount overlayfs.  On failure: umount2 /upper and /lower with
//         MNT_DETACH, then panic.  No retry — failure here is structural.
let opts = "lowerdir=/lower,upperdir=/upper/root,workdir=/upper/.work";
mount("overlay", "/merged", "overlay", MsFlags::empty(), Some(opts))?;

// ── Phase 5: virtual filesystems into merged (before pivot) ───────────────
// Step 7. Bind /proc, /sys, /dev INTO /merged/... so they are available
//         after pivot. /dev is recursive so the nested /dev/pts devpts
//         mount remains available for PTY allocation after pivot.
mount("/proc",  "/merged/proc",  None, MS_BIND | MS_REC, None)?;
mount("/sys",   "/merged/sys",   None, MS_BIND,           None)?;
mount("/dev",   "/merged/dev",   None, MS_BIND | MS_REC, None)?;

// ── Phase 6: pivot ────────────────────────────────────────────────────────
// Step 8. Make the current root MS_SLAVE so unmounts don't propagate to
//         the host.  Then bind /merged to itself (needed for pivot_root
//         when the new root is the same mount as the overlay).
mount(None, "/", None, MS_SLAVE | MS_REC, None)?;
mount("/merged", "/merged", None, MS_BIND | MS_REC, None)?;

// Step 9. pivot_rootfs("/merged") — lifted verbatim from kata-containers.
//         See §3.2 for the function body and attribution.
pivot_rootfs("/merged")?;

// ── Phase 7: workspace ────────────────────────────────────────────────────
// Step 10. Mount /dev/vdc at /workspace inside the new root.
//          Only when m80.workspace=1 says a workspace drive is present.
if cmdline_has("m80.workspace=1") && Path::new("/dev/vdc").exists() {
    fs::create_dir_all("/workspace")?;   // /workspace is provided by the base
    mount("/dev/vdc", "/workspace", "ext4", MsFlags::empty(), None)?;
}

// Step 11. Continue: exec the guest main loop / run the user command.
```

**Key invariants:**
- Steps 1 through 8 must execute before `pivot_rootfs` (step 9).
- `/lower`, `/upper`, and `/merged` must exist in the minimal base image. The initial root is mounted read-only, so PID 1 verifies these mountpoints rather than creating them at runtime.
- `/upper/root` and `/upper/.work` must be on the same superblock as each other (both on `/dev/vdb`). They must NOT be on `/dev/vda`.
- `workdir` must be empty at overlay mount time. A freshly formatted sparse ext4 guarantees this.
- The workspace mount (step 10) occurs INSIDE the pivoted root — after step 9, not before.

### 3.2 `pivot_rootfs` — verbatim lift from kata-containers

The following function is lifted verbatim from
`kata-containers/src/agent/rustjail/src/mount.rs:507-559`
with only the import context and the cfg-shim split retained.

```rust
// Adapted from kata-containers/src/agent/rustjail/src/mount.rs
// Copyright (c) 2019 Ant Financial
// SPDX-License-Identifier: Apache-2.0
// <https://github.com/kata-containers/kata-containers>

use nix::fcntl::{self, OFlag};
use nix::mount::{MntFlags, MsFlags, mount, umount2};
use nix::sys::stat::{self, Mode};
use nix::unistd;
use nix::NixPath;
use anyhow::{Context, Result};
// scopeguard = "1" in Cargo.toml; use scopeguard::defer;

/// Production path: calls the real pivot_root(2) syscall.
#[cfg(not(test))]
fn pivot_root<P1: ?Sized + NixPath, P2: ?Sized + NixPath>(
    new_root: &P1,
    put_old: &P2,
) -> Result<(), nix::Error> {
    unistd::pivot_root(new_root, put_old)
}

/// Test path: stubs the syscall so unit tests run without a mount namespace.
#[cfg(test)]
fn pivot_root<P1: ?Sized + NixPath, P2: ?Sized + NixPath>(
    _new_root: &P1,
    _put_old: &P2,
) -> Result<(), nix::Error> {
    Ok(())
}

pub fn pivot_rootfs<P: ?Sized + NixPath + std::fmt::Debug>(path: &P) -> Result<()> {
    let oldroot = fcntl::open("/", OFlag::O_DIRECTORY | OFlag::O_RDONLY, Mode::empty())?;
    defer!(unistd::close(oldroot).unwrap());
    let newroot = fcntl::open(path, OFlag::O_DIRECTORY | OFlag::O_RDONLY, Mode::empty())?;
    defer!(unistd::close(newroot).unwrap());

    // Change to the new root so that the pivot_root actually acts on it.
    unistd::fchdir(newroot)?;
    pivot_root(".", ".").context(format!("failed to pivot_root on {path:?}"))?;

    // Currently our "." is oldroot (according to the current kernel code).
    // However, purely for safety, we will fchdir(oldroot) since there isn't
    // really any guarantee from the kernel what /proc/self/cwd will be after a
    // pivot_root(2).
    unistd::fchdir(oldroot)?;

    // Make oldroot rslave to make sure our unmounts don't propagate to the
    // host. We don't use rprivate because this is known to cause issues due
    // to races where we still have a reference to a mount while a process in
    // the host namespace are trying to operate on something they think has no
    // mounts (devicemapper in particular).
    mount(
        Some("none"),
        ".",
        Some(""),
        MsFlags::MS_SLAVE | MsFlags::MS_REC,
        Some(""),
    )?;

    // Perform the unmount. MNT_DETACH allows us to unmount /proc/self/cwd.
    umount2(".", MntFlags::MNT_DETACH).context("failed to do umount2")?;

    // Switch back to our shiny new root.
    unistd::chdir("/")?;
    stat::umask(Mode::from_bits_truncate(0o022));
    Ok(())
}
```

**Step ordering rationale** (from `kata-containers.md §1`, local-source deep dive):

1. Both FDs opened before any `fchdir` — guards run cleanly on either open failure.
2. `fchdir(newroot)` before `pivot_root(".", ".")` — the `"."` argument resolves relative to cwd; kernel requires cwd inside the new root.
3. `pivot_root(".", ".")` — the runc "no separate put_old mountpoint" trick; the kernel stacks old root under new root momentarily.
4. `fchdir(oldroot)` for safety — uses pre-opened FD, not `/proc/self/cwd`, which is not contractually guaranteed after the call.
5. `MS_SLAVE | MS_REC` — prevents unmount propagating to host. `MS_SLAVE` not `MS_PRIVATE` to avoid known races with devicemapper (irrelevant to m80, but retained as authored).
6. `umount2(".", MNT_DETACH)` — lazy unmount because `/proc/self/cwd` still points at the directory being unmounted.
7. `chdir("/")` — move into the new root after old root is detached.
8. `umask(0o022)` — reset to sane default after namespace transition.

**Cargo.toml addition required** for `m80-guestd`:
```toml
scopeguard = "1"
```

---

## 4. Failure policy

**Overlay mount failure:** If any mount call in the PID-1 sequence fails (steps 2, 3, 6, 7, 8), m80-guestd must:

1. Log the failed step and errno to stderr (visible in Firecracker's serial console output).
2. Unmount successfully-mounted layers with `umount2(MNT_DETACH)` in reverse order (overlay first if mounted, then upper, then lower) to release the block devices cleanly.
3. Panic or exit with non-zero status.

There is no fallback to running on the bare lower layer. There is no retry. Failure at this stage is structural (wrong kernel config, malformed overlay image, or a bug in m80-guestd) — retrying hides it. Per CLAUDE.md "no silent recovery".

**Host-side effect of PID-1 panic:** The kernel panics (PID 1 exit triggers kernel panic). Firecracker surfaces this as the VM dying; m80-firecracker returns `FcError::GuestBootFailed { phase: "overlay-mount", errno }`. The run-dir is preserved for offline triage. The admission permit is dropped.

**`pivot_rootfs` failure:** Same policy — panic. The kata function body already propagates errors via `Result`; m80-guestd's caller unwraps and panics.

**No recovery from within the guest.** The minimal-init image has no secondary init to fall back to. The correct response to an unrecoverable boot failure is a fast, loud signal to the host.

---

## 5. Sparse overlay sizing

| Parameter | Value |
|---|---|
| Default overlay size | 512 MiB sparse |
| Configuration field | `SandboxConfig::overlay_size_bytes` |
| Clone policy field | `SandboxConfig::overlay_clone_mode` |
| Allocation mechanism | `File::create(overlay_dest)? + file.set_len(overlay_size_bytes)?` |
| Format | `mkfs.ext4 -F <overlay_dest>` shell-out |
| On-disk cost at creation | Zero bytes (sparse on ext4, xfs, btrfs, tmpfs) |
| On-disk cost at runtime | O(actual guest writes) |
| ENOSPC behavior | Surfaces as a normal write error inside the guest. Not a launch issue. Document in `m80-ovrl.8` README: "VMs are short-lived; overlay sized for the working set, not for accumulation." Persistent-VM mode (`m80-qokt.2`) re-evaluates. |

**Host filesystem assumption:** m80's run-dir is on a Linux native filesystem (ext4, xfs, btrfs, tmpfs). Sparse file support is guaranteed on all four. If the run-dir is on a FUSE or non-sparse-supporting filesystem, `File::set_len` may pre-allocate, stalling launch. A `statfs`-based preflight check is deferred as a follow-up (see R6 in §6); it is not blocking for v0.1.

**Upper/workdir chronology** (from `storage-pivot-bead-plan.md §3` Q4):

1. **Host launch:** `Rootfs::prepare` creates the sparse file and runs `mkfs.ext4 -F` → fresh empty ext4.
2. **Guest PID 1:** mounts `/dev/vdb` at `/upper`.
3. **Guest PID 1:** `mkdir /upper/root` and `mkdir /upper/.work`.
4. **Guest PID 1:** mounts overlayfs with these as upper/workdir.

The workdir is empty at overlay mount time by construction (freshly formatted ext4). Step 3 cannot happen earlier (pre-population at image-build time would put it on the base/lower, not the upper, violating the same-superblock requirement).

---

## 6. Risk register

Copied and distilled from `docs/planning/perf-roadmap-extended.md §1.1`. All seven risks are tracked at the design level; mitigations reference the leaves that own the work.

| # | Failure mode | Detection | Mitigation |
|---|---|---|---|
| R1 | firecracker-ci 5.10.245 kernel ships **without** `CONFIG_OVERLAY_FS=y` (built-in). Module form (`=m`) is insufficient — module loading is not up when PID 1 runs. | `m80-ovrl.5` IKCFG grep; runtime `cat /proc/filesystems \| grep overlay`. | `m80-ovrl.5` escalates from "verify" to "build custom kernel" with `CONFIG_OVERLAY_FS=y` + `CONFIG_OVERLAY_FS_XINO_AUTO=y`. Pre-empted by `m80-ci9i.1` keep-list (see cross-epic constraint below). |
| R2 | `pivot_root(".", ".")` fails as PID 1 (e.g., mount propagation not set to `MS_PRIVATE` before the call). | Guest panics; `phase_12b_ready_accept` times out at 60 s; run-dir preserved. | Step 1 of the in-guest sequence (`MS_REC \| MS_PRIVATE` on `/`) must execute before step 9. `m80-ovrl.4` integration test boots end-to-end on CI's kernel. |
| R3 | kata `pivot_rootfs` lift has a subtle ordering bug (e.g., `scopeguard` `defer!` drop order differs). | `pid_one_pivot` unit test stubs the syscall; real failure only in integration. | Smoke checkpoint `m80-f2zc.5b`: single end-to-end launch with `M80_PHASE_TRACE=1`, asserts probe-after-pivot byte lands on overlay disk. |
| R4 | RO base page cache is NOT shared across VMs (e.g., host filesystem opens a fresh inode). | 16-VM concurrent bench shows `/proc/meminfo` Cached delta > 256 MiB × 16 ÷ 4. | `m80-preflight` rejects non-native run-root filesystems (deferred follow-up). `m80-ovrl.7` bench records Cached delta as R4 data point. |
| R5 | Template clone remains slow (>80 ms) or falls back to full copy on the target filesystem. | `phase_3b_rootfs_prepare` exceeds 80 ms in bench after `m80-f2zc.10`. | Current data shows template clone at about 13.8 ms P50; do not introduce a broader overlay artifact pool from total storage-prep time unless rootfs-prepare itself regresses. |
| R6 | Sparse `File::set_len` pre-allocates on a FUSE or non-sparse host filesystem, stalling launch. | Storage prep time regresses to O(overlay size). | Run-dir on Linux native filesystem is the documented requirement. `statfs`-based preflight check deferred. |
| R7 | Base file mutated post-launch by concurrent `m80-image-build` re-run. | sha256 mismatch on next launch. | `m80-image-manifest` verifies before mount. Smoke checkpoint `m80-f2zc.5b` hashes base pre/post launch and asserts equality. |

---

## 7. Cross-epic constraint: CONFIG_OVERLAY_FS=y in the stripped kernel

**Non-negotiable:** `CONFIG_OVERLAY_FS=y` MUST appear in the stripped kernel keep-list produced by `m80-ci9i.1` (stripped kernel DESIGN). This is the single most critical cross-epic constraint in the perf roadmap:

- If `m80-ci9i` ships a stripped kernel without this symbol, the overlayfs mount in PID 1 (step 6) will fail with `ENODEV` or `ENOSYS`, the guest will panic, and every VM launch fails.
- `CONFIG_OVERLAY_FS_XINO_AUTO=y` must also be in the keep-list (`overlayfs-kernel-semantics.md §5` — provides correct `st_ino` semantics across two ext4 superblocks at no runtime cost).
- This constraint is tracked as cross-epic dep `m80-f2zc.5 → m80-ci9i.1` (related) in the bead plan.

The dependency tracks: `m80-ovrl.5` (kernel config verification for the current stock kernel) must be completed before `m80-ci9i.1` DESIGN locks the stripped kernel keep-list. If `m80-ovrl.5` finds the stock kernel already has `CONFIG_OVERLAY_FS=y`, that information informs `m80-ci9i.1`. If not, `m80-ci9i` must build the symbol in regardless.

---

## 8. Measured impact

Measured on 2026-05-05 against a freshly rebuilt minimal image at
`/tmp/m80-build/minimal-perf-20260505c` (Firecracker 1.15.1, Linux
6.17.0-22-generic host).

| Metric | Baseline (clone) | Post-pivot | Delta |
|---|---|---|---|
| `storage_prep` p50 | 727.6 ms | 215.9 ms | -511.7 ms |
| `ready_accept` p50 | 893.4 ms | 906.2 ms | +12.8 ms |
| minimal/idle useful p50 | 1696 ms | 1207 ms | -489 ms |
| 16-VM concurrent `/proc/meminfo` Cached delta | not measured | +4.4 MiB | shared-base cache behavior looks healthy |
| stress-ng 100% CPU success rate | 0/30 | 1/5 | still mostly failing; orthogonal to storage |

The storage pivot removed the full 256 MiB rootfs copy from the launch path,
but sparse overlay creation plus `mkfs.ext4` still costs about 216 ms p50. The
loaded-cell failures persist after the pivot, so the saturation issue remains
vsock/scheduling work rather than storage-copy work.

Follow-up `m80-f2zc.10` replaced per-launch `mkfs.ext4` with a run-root-local
empty overlay template cloned through the gated `cp --reflink=always` /
`cp --reflink=never` path. That gate is now an explicit clone policy rather
than a fallback path.
Measured on 2026-05-05:

| Metric | Minimal stock idle | Minimal stripped idle |
|---|---:|---:|
| `phase_3_storage_prep` P50 | 180.6 ms | 180.5 ms |
| `phase_3a_manifest_verify` P50 | 166.6 ms | 166.8 ms |
| `phase_3b_rootfs_prepare` P50 | 13.7 ms | 13.8 ms |

The overlay template removed the material rootfs-prepare cost. The remaining
storage-prep residual is manifest sha256 verification, not overlay image
creation; any further 100 ms storage win must preserve that fail-closed boot
artifact invariant explicitly. Follow-up: `m80-f2zc.11`.

`m80-f2zc.11` resolves that residual by using the existing preflight
`Rootfs + manifest` check as the boot-artifact trust boundary. Phase 3 no
longer rehashes kernel/rootfs/guestd artifacts for each VM; it prepares the
overlay and optional scratch only. Minimal stripped idle moved from
`phase_3_storage_prep` 180.5 ms P50 to 13.4 ms P50, and wallclock moved from
1317 ms P50 to 1117 ms P50 on the same 2026-05-05 bench host.

---

## 9. Leaf dependency summary

```
m80-ovrl.1  (this doc — DESIGN)
  ├─ m80-ovrl.2   m80-storage IMPL (Rootfs::prepare, delete clone)
  │    └─ m80-ovrl.3   m80-firecracker IMPL (3-drive PUT order)
  │         └─ m80-ovrl.6   tests
  ├─ m80-ovrl.5   image-build kernel CONFIG_OVERLAY_FS verify
  │    └─ m80-ovrl.4   m80-guestd IMPL (overlayfs + pivot_root in PID 1)
  │         ├─→ m80-ovrl.6   tests
  │         └─→ m80-ovrl.7   bench
  │                   └─→ m80-ovrl.8   docs
  └─ m80-ovrl.4   also depends on m80-ovrl.1 (this doc)

m80-ovrl.4a  workspace mount /dev/vdb → /dev/vdc (follow-up under m80-6a0q.3)
  depends on: m80-ovrl.4
```

Cross-epic:
- `m80-ovrl.5` is a related dep of `m80-ci9i.1` (stripped kernel DESIGN must include `CONFIG_OVERLAY_FS=y`).
- `m80-rrp.3.4` (snapshot launch path) should be tested with the post-pivot drive layout; tracked as related dep.
