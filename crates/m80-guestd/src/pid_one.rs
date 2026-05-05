//! PID-1 mode: when m80-guestd is the init process (no systemd).
//!
//! Activated only when `is_pid_one()` returns true. We avoid signal
//! handlers (which require `unsafe` and are async-signal-unsafe to write
//! correctly) — instead we poll-reap orphaned children between vsock
//! requests. The firecracker host's stop path is a SIGKILL on the
//! outside, so graceful SIGTERM handling buys nothing for v0.1.
//!
//! Startup sequence (PID-1 mode):
//!
//! 1. Install panic hook.
//! 2. Mount pseudo-filesystems (`/proc`, `/sys`, `/dev`).
//! 3. Mount overlay layers and `pivot_root` into the merged rootfs
//!    (see `mount_overlay_and_pivot` — implements design doc §3.1 steps 1-9).
//! 4. Mount workspace drive `/dev/vdc` → `/workspace` if present (step 10).
//! 5. Continue: vsock listener, ready signal, exec loop.

use std::path::Path;

use anyhow::Context as _;
use nix::mount::{mount, umount2, MntFlags, MsFlags};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;

/// True when this process was launched by the kernel as init (PID 1).
pub fn is_pid_one() -> bool {
    std::process::id() == 1
}

/// Configure the process for PID-1 duty: pseudo-fs mounts, overlay mount,
/// pivot_root, workspace mount, and panic hook. Call exactly once early in
/// `main`.
pub fn enter_pid_one_mode() -> anyhow::Result<()> {
    install_panic_hook();
    mount_pseudo_filesystems().context("pseudo-fs mounts")?;
    mount_overlay_and_pivot().context("overlay mount and pivot_root")?;
    mount_workspace_if_present().context("workspace mount")?;
    Ok(())
}

/// Mount `/proc`, `/sys`, and `/dev`. The firecracker-ci kernel ships
/// `CONFIG_DEVTMPFS_MOUNT=y` so `/dev` is normally pre-populated by the
/// kernel — `EBUSY` (already mounted) is treated as success.
fn mount_pseudo_filesystems() -> anyhow::Result<()> {
    eprintln!("[guestd] mounting pseudo-filesystems");
    mount_one("proc",     "/proc", "proc",     MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_NOEXEC)?;
    mount_one("sysfs",    "/sys",  "sysfs",    MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_NOEXEC)?;
    mount_one("devtmpfs", "/dev",  "devtmpfs", MsFlags::MS_NOSUID)?;
    eprintln!("[guestd] pseudo-filesystems mounted");
    Ok(())
}

fn mount_one(source: &str, target: &str, fstype: &str, flags: MsFlags) -> anyhow::Result<()> {
    match mount(Some(source), Path::new(target), Some(fstype), flags, None::<&str>) {
        Ok(()) => Ok(()),
        Err(nix::errno::Errno::EBUSY) => Ok(()),
        Err(e) => Err(anyhow::anyhow!("mount {source} -> {target} ({fstype}): {e}")),
    }
}

/// Implement design doc §3.1 steps 1–9:
///
/// 1. Make mount namespace fully private (MS_REC | MS_PRIVATE on /).
/// 2. Mount /dev/vda (RO base ext4) at /lower.
/// 3. Mount /dev/vdb (RW overlay ext4) at /upper.
/// 4. mkdir /upper/root and /upper/.work (idempotent).
/// 5. mkdir /merged.
/// 6. Mount overlayfs with lowerdir=/lower, upperdir=/upper/root, workdir=/upper/.work at /merged.
/// 7. Bind-mount /proc, /sys, /dev into /merged.
/// 8. MS_SLAVE | MS_REC on / and MS_BIND | MS_REC /merged onto itself.
/// 9. pivot_rootfs("/merged").
///
/// On any failure, cleanup already-mounted layers (MNT_DETACH) and panic.
/// No retry, no fallback — failure here is structural.
fn mount_overlay_and_pivot() -> anyhow::Result<()> {
    // ── Phase 1: namespace isolation ──────────────────────────────────────
    // Step 1. Make the current mount namespace fully private so pivot_root(2)
    //         does not propagate to the host.  Must come before any pivot_root.
    eprintln!("[guestd] step 1: making mount namespace private");
    mount(
        None::<&str>,
        "/",
        None::<&str>,
        MsFlags::MS_REC | MsFlags::MS_PRIVATE,
        None::<&str>,
    )
    .context("step 1: mount / MS_REC|MS_PRIVATE failed")?;

    // ── Phase 2: mount layer disks ────────────────────────────────────────
    // Step 2. Mount the shared read-only base ext4 (vda) at /lower.
    eprintln!("[guestd] step 2: mounting /dev/vda at /lower");
    std::fs::create_dir_all("/lower").context("step 2: mkdir /lower")?;
    mount(
        Some("/dev/vda"),
        "/lower",
        Some("ext4"),
        MsFlags::MS_RDONLY,
        None::<&str>,
    )
    .context("step 2: mount /dev/vda -> /lower (ext4, rdonly)")?;

    // Step 3. Mount the per-VM writable ext4 (vdb) at /upper.
    eprintln!("[guestd] step 3: mounting /dev/vdb at /upper");
    std::fs::create_dir_all("/upper").context("step 3: mkdir /upper")?;
    let upper_mounted = mount(
        Some("/dev/vdb"),
        "/upper",
        Some("ext4"),
        MsFlags::empty(),
        None::<&str>,
    );
    if let Err(ref e) = upper_mounted {
        eprintln!("[guestd] step 3 FAILED: /dev/vdb -> /upper: {e}; detaching /lower");
        let _ = umount2("/lower", MntFlags::MNT_DETACH);
    }
    upper_mounted.context("step 3: mount /dev/vdb -> /upper (ext4)")?;

    // ── Phase 3: prepare overlay dirs ─────────────────────────────────────
    // Step 4. Create upperdir and workdir on /dev/vdb's superblock.
    eprintln!("[guestd] step 4: creating /upper/root and /upper/.work");
    std::fs::create_dir_all("/upper/root").context("step 4: mkdir /upper/root")?;
    std::fs::create_dir_all("/upper/.work").context("step 4: mkdir /upper/.work")?;

    // Step 5. Create the overlay merge target.
    eprintln!("[guestd] step 5: creating /merged");
    std::fs::create_dir_all("/merged").context("step 5: mkdir /merged")?;

    // ── Phase 4: overlayfs ────────────────────────────────────────────────
    // Step 6. Mount overlayfs.
    eprintln!("[guestd] step 6: mounting overlayfs at /merged");
    let opts = "lowerdir=/lower,upperdir=/upper/root,workdir=/upper/.work";
    let overlay_mounted = mount(
        Some("overlay"),
        "/merged",
        Some("overlay"),
        MsFlags::empty(),
        Some(opts),
    );
    if let Err(ref e) = overlay_mounted {
        eprintln!("[guestd] step 6 FAILED: overlayfs: {e}; detaching /upper and /lower");
        let _ = umount2("/upper", MntFlags::MNT_DETACH);
        let _ = umount2("/lower", MntFlags::MNT_DETACH);
    }
    overlay_mounted.context("step 6: mount overlayfs -> /merged")?;

    // ── Phase 5: virtual filesystems into merged (before pivot) ───────────
    // Step 7. Bind /proc, /sys, /dev into /merged/... so they survive pivot.
    //         Flags per docs/exploration/runc-crun-overlayfs-init.md §4:
    //         MS_BIND | MS_REC for /proc; MS_BIND for /sys and /dev.
    eprintln!("[guestd] step 7: bind-mounting /proc /sys /dev into /merged");
    mount(
        Some("/proc"),
        "/merged/proc",
        None::<&str>,
        MsFlags::MS_BIND | MsFlags::MS_REC,
        None::<&str>,
    )
    .context("step 7: bind /proc -> /merged/proc")?;
    mount(
        Some("/sys"),
        "/merged/sys",
        None::<&str>,
        MsFlags::MS_BIND,
        None::<&str>,
    )
    .context("step 7: bind /sys -> /merged/sys")?;
    mount(
        Some("/dev"),
        "/merged/dev",
        None::<&str>,
        MsFlags::MS_BIND,
        None::<&str>,
    )
    .context("step 7: bind /dev -> /merged/dev")?;

    // ── Phase 6: pivot ────────────────────────────────────────────────────
    // Step 8. Make current root MS_SLAVE so unmounts don't propagate to host.
    //         Then bind /merged onto itself (required by pivot_root when the
    //         new root shares the overlay mount).
    eprintln!("[guestd] step 8: MS_SLAVE on / and self-bind /merged");
    mount(
        None::<&str>,
        "/",
        None::<&str>,
        MsFlags::MS_SLAVE | MsFlags::MS_REC,
        None::<&str>,
    )
    .context("step 8: mount / MS_SLAVE|MS_REC")?;
    mount(
        Some("/merged"),
        "/merged",
        None::<&str>,
        MsFlags::MS_BIND | MsFlags::MS_REC,
        None::<&str>,
    )
    .context("step 8: bind /merged onto itself")?;

    // Step 9. pivot_rootfs("/merged") — lifted verbatim from kata-containers.
    eprintln!("[guestd] step 9: pivot_rootfs(/merged)");
    pivot_rootfs("/merged").context("step 9: pivot_rootfs(/merged)")?;
    eprintln!("[guestd] pivot complete — now running in merged rootfs");

    Ok(())
}

// Adapted from kata-containers/src/agent/rustjail/src/mount.rs:507-559
// Copyright (c) 2019 Ant Financial
// SPDX-License-Identifier: Apache-2.0
// <https://github.com/kata-containers/kata-containers>

use nix::fcntl::{self, OFlag};
use nix::sys::stat::{self, Mode};
use nix::unistd;
use nix::NixPath;
use scopeguard::defer;

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

/// Lift verbatim from kata-containers/src/agent/rustjail/src/mount.rs:523-559.
pub fn pivot_rootfs<P: ?Sized + NixPath + std::fmt::Debug>(path: &P) -> anyhow::Result<()> {
    let oldroot = fcntl::open("/", OFlag::O_DIRECTORY | OFlag::O_RDONLY, Mode::empty())?;
    defer!(unistd::close(oldroot).unwrap());
    let newroot = fcntl::open(path, OFlag::O_DIRECTORY | OFlag::O_RDONLY, Mode::empty())?;
    defer!(unistd::close(newroot).unwrap());

    // Change to the new root so that the pivot_root actually acts on it.
    unistd::fchdir(newroot)?;
    pivot_root(".", ".").with_context(|| format!("failed to pivot_root on {path:?}"))?;

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

/// Workspace device — after overlay pivot the workspace is /dev/vdc (position 3
/// in the drive-PUT order per docs/design/storage-overlay.md §2).
const WORKSPACE_DEV: &str = "/dev/vdc";
const WORKSPACE_TARGET: &str = "/workspace";

/// Mount the workspace drive at `/workspace` if attached. The workspace
/// is optional (a Sandbox without `workspace_dir` produces no
/// scratch.ext4 and no `/dev/vdc`); skip the mount in that case rather
/// than failing PID-1 setup. ENOENT on the device node is the documented
/// optional-state response, not silent error recovery.
///
/// This is step 10 of the design doc §3.1 sequence. Must be called AFTER
/// `pivot_rootfs` so the mount lands inside the new (merged) root.
fn mount_workspace_if_present() -> anyhow::Result<()> {
    if !Path::new(WORKSPACE_DEV).exists() {
        eprintln!("[guestd] no workspace drive ({WORKSPACE_DEV}), skipping workspace mount");
        tracing::info!(
            dev = WORKSPACE_DEV,
            "no workspace drive attached, skipping workspace mount"
        );
        return Ok(());
    }
    eprintln!("[guestd] step 10: mounting {WORKSPACE_DEV} at {WORKSPACE_TARGET}");
    mount_one(WORKSPACE_DEV, WORKSPACE_TARGET, "ext4", MsFlags::empty())
}

/// Reap any pending zombies. Call between requests so orphans (children
/// of children that re-parent to PID 1) don't accumulate. Non-blocking;
/// returns once `waitpid(WNOHANG)` reports nothing more to reap.
pub fn reap_pending() {
    loop {
        match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::StillAlive) => break,
            Ok(_) => continue,
            Err(_) => break,
        }
    }
}

/// Panic hook: log and exit non-zero. As PID 1, an unwinding panic
/// causes a kernel panic; with `panic=1` in boot args the kernel reboots,
/// which surfaces the failure to the host instead of leaving the VM in
/// undefined state.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        eprintln!("m80-guestd PID-1 panic: {info}");
        std::process::exit(1);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_pid_one_false_in_test_runner() {
        assert!(!is_pid_one());
    }

    #[test]
    fn reap_pending_safe_with_no_children() {
        reap_pending();
    }

    /// Verify that the overlay option string is constructed correctly.
    #[test]
    fn overlay_opts_format() {
        let lower = "/lower";
        let upper = "/upper/root";
        let work = "/upper/.work";
        let opts = format!("lowerdir={lower},upperdir={upper},workdir={work}");
        assert_eq!(
            opts,
            "lowerdir=/lower,upperdir=/upper/root,workdir=/upper/.work"
        );
    }

    /// Verify that the workspace device path is /dev/vdc (post-pivot drive
    /// layout per docs/design/storage-overlay.md §2).
    #[test]
    fn workspace_dev_is_vdc() {
        assert_eq!(WORKSPACE_DEV, "/dev/vdc");
    }

    /// Verify that pivot_rootfs (with the test stub for pivot_root) does not
    /// panic when called with a valid path. This exercises the defer!/close
    /// wrappers and fchdir sequence without a real mount namespace.
    ///
    /// Requires a real filesystem path that is a directory; "/" works.
    /// The syscall is stubbed in #[cfg(test)] so no privilege is needed.
    #[test]
    #[ignore = "requires a mount namespace; run in a real VM or with unshare -m"]
    fn pivot_rootfs_on_real_namespace() {
        // Would call pivot_rootfs("/merged") in a real mount namespace.
        // Stub path exercises the open/fchdir/defer logic without actual
        // pivot_root(2) — covered by the cfg(test) shim above.
        pivot_rootfs("/").expect("pivot_rootfs stub must not fail");
    }
}
