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
//! 4. Mount workspace drive `/dev/vdc` → `/workspace` if present (step 11).
//! 5. Continue: vsock listener, ready signal, exec loop.

use std::fs::OpenOptions;
use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::process::Command;

use anyhow::Context as _;
use nix::mount::{mount, umount2, MntFlags, MsFlags};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::{dup2, Pid};

use crate::guest_log::{self, BootTimer, GuestLogPhase};

/// True when this process was launched by the kernel as init (PID 1).
pub(crate) fn is_pid_one() -> bool {
    std::process::id() == 1
}

/// Configure the process for PID-1 duty: pseudo-fs mounts, overlay mount,
/// pivot_root, workspace mount, and panic hook. Call exactly once early in
/// `main`.
pub(crate) fn enter_pid_one_mode(boot_timer: &mut BootTimer) -> anyhow::Result<()> {
    redirect_stdio_to_console().context("redirect stdio to /dev/console")?;
    boot_timer.mark("stdio_redirected");
    install_panic_hook();
    boot_timer.mark("panic_hook_installed");
    mount_pseudo_filesystems().context("pseudo-fs mounts")?;
    boot_timer.mark("pseudo_fs_mounted");
    mount_overlay_and_pivot(boot_timer).context("overlay mount and pivot_root")?;
    boot_timer.mark("overlay_pivot_complete");
    crate::pid_one_network::configure_from_proc_cmdline().context("outbound network setup")?;
    boot_timer.mark("network_configured");
    mount_workspace_if_present(boot_timer).context("workspace mount")?;
    boot_timer.mark("pid1_setup_complete");
    Ok(())
}

/// Mount `/proc`, `/sys`, `/dev`, `/dev/shm`, and `/dev/pts`. The firecracker-ci kernel ships
/// `CONFIG_DEVTMPFS_MOUNT=y` so `/dev` is normally pre-populated by the
/// kernel — `EBUSY` (already mounted) is treated as success.
fn mount_pseudo_filesystems() -> anyhow::Result<()> {
    guest_log::info(GuestLogPhase::Boot, None, "mounting pseudo-filesystems");
    mount_one(
        "proc",
        "/proc",
        "proc",
        MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_NOEXEC,
    )?;
    mount_one(
        "sysfs",
        "/sys",
        "sysfs",
        MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_NOEXEC,
    )?;
    mount_one("devtmpfs", "/dev", "devtmpfs", MsFlags::MS_NOSUID)?;
    std::fs::create_dir_all(DEV_SHM_TARGET).context("mkdir /dev/shm")?;
    mount_one_with_data(
        "tmpfs",
        DEV_SHM_TARGET,
        "tmpfs",
        MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_NOEXEC,
        Some(DEV_SHM_MOUNT_DATA),
    )?;
    std::fs::create_dir_all(DEV_PTS_TARGET).context("mkdir /dev/pts")?;
    mount_one_with_data(
        "devpts",
        DEV_PTS_TARGET,
        "devpts",
        devpts_mount_flags(),
        Some(DEV_PTS_MOUNT_DATA),
    )?;
    ensure_ptmx_symlink().context("ensure /dev/ptmx -> /dev/pts/ptmx")?;
    guest_log::info(GuestLogPhase::Boot, None, "pseudo-filesystems mounted");
    Ok(())
}

/// In PID-1 mode systemd is not present, so `StandardError=journal+console`
/// cannot help us. Duplicate both stdout and stderr onto `/dev/console`
/// before any startup logs are emitted.
fn redirect_stdio_to_console() -> anyhow::Result<()> {
    let console = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/console")
        .context("open /dev/console")?;
    dup2(console.as_raw_fd(), 1).context("dup2 stdout -> /dev/console")?;
    dup2(console.as_raw_fd(), 2).context("dup2 stderr -> /dev/console")?;
    Ok(())
}

fn mount_one(source: &str, target: &str, fstype: &str, flags: MsFlags) -> anyhow::Result<()> {
    mount_one_with_data(source, target, fstype, flags, None)
}

fn mount_one_with_data(
    source: &str,
    target: &str,
    fstype: &str,
    flags: MsFlags,
    data: Option<&str>,
) -> anyhow::Result<()> {
    match mount(Some(source), Path::new(target), Some(fstype), flags, data) {
        Ok(()) => Ok(()),
        Err(nix::errno::Errno::EBUSY) => Ok(()),
        Err(e) => Err(anyhow::anyhow!(
            "mount {source} -> {target} ({fstype}): {e}"
        )),
    }
}

fn ensure_ptmx_symlink() -> anyhow::Result<()> {
    let ptmx = Path::new("/dev/ptmx");
    if let Ok(target) = std::fs::read_link(ptmx) {
        if target == Path::new("pts/ptmx") || target == Path::new("/dev/pts/ptmx") {
            return Ok(());
        }
    }
    match std::fs::remove_file(ptmx) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).context("remove existing /dev/ptmx"),
    }
    std::os::unix::fs::symlink("pts/ptmx", ptmx).context("symlink /dev/ptmx")
}

/// Implement design doc §3.1 steps 1–9:
///
/// 1. Make mount namespace fully private (MS_REC | MS_PRIVATE on /).
/// 2. Verify image-built mountpoints exist, then mount /dev/vda (RO base ext4) at /lower.
/// 3. Verify image-built mountpoints exist, then mount /dev/vdb (RW overlay ext4) at /upper.
/// 4. mkdir /upper/root and /upper/.work (idempotent).
/// 5. mkdir /merged.
/// 6. Mount overlayfs with lowerdir=/lower, upperdir=/upper/root, workdir=/upper/.work at /merged.
/// 7. Bind-mount /proc, /sys, /dev into /merged.
/// 8. MS_SLAVE | MS_REC on / and MS_BIND | MS_REC /merged onto itself.
/// 9. pivot_rootfs("/merged").
///
/// On any failure, cleanup already-mounted layers (MNT_DETACH) and panic.
/// No retry, no fallback — failure here is structural.
fn mount_overlay_and_pivot(boot_timer: &mut BootTimer) -> anyhow::Result<()> {
    // ── Phase 1: namespace isolation ──────────────────────────────────────
    // Step 1. Make the current mount namespace fully private so pivot_root(2)
    //         does not propagate to the host.  Must come before any pivot_root.
    guest_log::info(
        GuestLogPhase::Boot,
        None,
        "step 1: making mount namespace private",
    );
    mount(
        None::<&str>,
        "/",
        None::<&str>,
        MsFlags::MS_REC | MsFlags::MS_PRIVATE,
        None::<&str>,
    )
    .context("step 1: mount / MS_REC|MS_PRIVATE failed")?;
    boot_timer.mark("mount_namespace_private");

    // ── Phase 2: mount layer disks ────────────────────────────────────────
    // Step 2. Mount the shared read-only base ext4 (vda) at /lower.
    // / is already read-only, so these mountpoints are an image-build
    // contract, not something PID 1 can create at runtime.
    guest_log::info(
        GuestLogPhase::Boot,
        None,
        "step 2: mounting /dev/vda at /lower",
    );
    ensure_precreated_mountpoint("/lower").context("step 2: /lower mountpoint")?;
    mount(
        Some("/dev/vda"),
        "/lower",
        Some("ext4"),
        MsFlags::MS_RDONLY,
        None::<&str>,
    )
    .context("step 2: mount /dev/vda -> /lower (ext4, rdonly)")?;
    boot_timer.mark("base_mounted");

    // Step 3. Mount the per-VM writable ext4 (vdb) at /upper.
    guest_log::info(
        GuestLogPhase::Boot,
        None,
        "step 3: mounting /dev/vdb at /upper",
    );
    ensure_precreated_mountpoint("/upper").context("step 3: /upper mountpoint")?;
    let upper_mounted = mount(
        Some("/dev/vdb"),
        "/upper",
        Some("ext4"),
        MsFlags::empty(),
        None::<&str>,
    );
    if let Err(ref e) = upper_mounted {
        guest_log::error(
            GuestLogPhase::Boot,
            None,
            format!("step 3 FAILED: /dev/vdb -> /upper: {e}; detaching /lower"),
        );
        let _ = umount2("/lower", MntFlags::MNT_DETACH);
    }
    upper_mounted.context("step 3: mount /dev/vdb -> /upper (ext4)")?;
    boot_timer.mark("overlay_disk_mounted");

    // ── Phase 3: prepare overlay dirs ─────────────────────────────────────
    // Step 4. Create upperdir and workdir on /dev/vdb's superblock.
    guest_log::info(
        GuestLogPhase::Boot,
        None,
        "step 4: creating /upper/root and /upper/.work",
    );
    std::fs::create_dir_all("/upper/root").context("step 4: mkdir /upper/root")?;
    std::fs::create_dir_all("/upper/.work").context("step 4: mkdir /upper/.work")?;
    boot_timer.mark("overlay_dirs_ready");

    // Step 5. Create the overlay merge target.
    guest_log::info(GuestLogPhase::Boot, None, "step 5: creating /merged");
    ensure_precreated_mountpoint("/merged").context("step 5: /merged mountpoint")?;
    boot_timer.mark("merged_mountpoint_ready");

    // ── Phase 4: overlayfs ────────────────────────────────────────────────
    // Step 6. Mount overlayfs.
    guest_log::info(
        GuestLogPhase::Boot,
        None,
        "step 6: mounting overlayfs at /merged",
    );
    let opts = "lowerdir=/lower,upperdir=/upper/root,workdir=/upper/.work";
    let overlay_mounted = mount(
        Some("overlay"),
        "/merged",
        Some("overlay"),
        MsFlags::empty(),
        Some(opts),
    );
    if let Err(ref e) = overlay_mounted {
        guest_log::error(
            GuestLogPhase::Boot,
            None,
            format!("step 6 FAILED: overlayfs: {e}; detaching /upper and /lower"),
        );
        let _ = umount2("/upper", MntFlags::MNT_DETACH);
        let _ = umount2("/lower", MntFlags::MNT_DETACH);
    }
    overlay_mounted.context("step 6: mount overlayfs -> /merged")?;
    boot_timer.mark("overlayfs_mounted");

    // ── Phase 5: virtual filesystems into merged (before pivot) ───────────
    // Step 7. Bind /proc, /sys, /dev into /merged/... so they survive pivot.
    //         /dev must be recursive because /dev/pts is a nested devpts mount
    //         required by PTY allocation after pivot.
    guest_log::info(
        GuestLogPhase::Boot,
        None,
        "step 7: bind-mounting /proc /sys /dev into /merged",
    );
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
        dev_bind_flags(),
        None::<&str>,
    )
    .context("step 7: bind /dev -> /merged/dev")?;
    boot_timer.mark("pseudo_fs_bound_into_merged");

    // ── Phase 6: pivot ────────────────────────────────────────────────────
    // Step 8. Make current root MS_SLAVE so unmounts don't propagate to host.
    //         Then bind /merged onto itself (required by pivot_root when the
    //         new root shares the overlay mount).
    guest_log::info(
        GuestLogPhase::Boot,
        None,
        "step 8: MS_SLAVE on / and self-bind /merged",
    );
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
    boot_timer.mark("merged_self_bound");

    // Step 9. pivot_rootfs("/merged") — lifted verbatim from kata-containers.
    guest_log::info(GuestLogPhase::Boot, None, "step 9: pivot_rootfs(/merged)");
    pivot_rootfs("/merged").context("step 9: pivot_rootfs(/merged)")?;
    boot_timer.mark("pivot_rootfs_done");
    guest_log::info(
        GuestLogPhase::Boot,
        None,
        "pivot complete; now running in merged rootfs",
    );

    Ok(())
}

fn ensure_precreated_mountpoint(path: &str) -> anyhow::Result<()> {
    let metadata = std::fs::metadata(path).with_context(|| format!("{path} must exist"))?;
    if !metadata.is_dir() {
        anyhow::bail!("{path} must be a directory");
    }
    Ok(())
}

fn dev_bind_flags() -> MsFlags {
    MsFlags::MS_BIND | MsFlags::MS_REC
}

fn devpts_mount_flags() -> MsFlags {
    MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC
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
pub(crate) fn pivot_rootfs<P: ?Sized + NixPath + std::fmt::Debug>(path: &P) -> anyhow::Result<()> {
    let oldroot = fcntl::open("/", OFlag::O_DIRECTORY | OFlag::O_RDONLY, Mode::empty())?;
    defer!{ if let Err(e) = unistd::close(oldroot) { guest_log::warn(GuestLogPhase::Boot, None, &format!("close(oldroot): {e}")); } }
    let newroot = fcntl::open(path, OFlag::O_DIRECTORY | OFlag::O_RDONLY, Mode::empty())?;
    defer!{ if let Err(e) = unistd::close(newroot) { guest_log::warn(GuestLogPhase::Boot, None, &format!("close(newroot): {e}")); } }

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
const DEV_SHM_TARGET: &str = "/dev/shm";
const DEV_PTS_TARGET: &str = "/dev/pts";
const DEV_SHM_MOUNT_DATA: &str = "mode=1777";
const DEV_PTS_MOUNT_DATA: &str = "gid=5,mode=620,ptmxmode=666";
const WORKSPACE_CMDLINE_FLAG: &str = "m80.workspace=1";
const WORKSPACE_MKFS_CMDLINE_FLAG: &str = "m80.workspace.mkfs=1";

/// Mount the workspace drive at `/workspace` if attached. The workspace
/// is optional (a Sandbox without `workspace_dir` produces no
/// scratch.ext4 and no `/dev/vdc`); skip the mount in that case rather
/// than failing PID-1 setup. ENOENT on the device node is the documented
/// optional-state response, not silent error recovery.
///
/// This is step 11 of the design doc §3.1 sequence. Must be called AFTER
/// `pivot_rootfs` so the mount lands inside the new (merged) root.
fn mount_workspace_if_present(boot_timer: &mut BootTimer) -> anyhow::Result<()> {
    if !workspace_requested()? {
        guest_log::info(
            GuestLogPhase::Boot,
            None,
            "workspace drive not requested, skipping workspace mount",
        );
        boot_timer.mark("workspace_absent");
        return Ok(());
    }
    mount_workspace_device_if_present(
        boot_timer,
        WORKSPACE_DEV,
        WORKSPACE_TARGET,
        workspace_mkfs_allowed()?,
    )
}

fn workspace_requested() -> anyhow::Result<bool> {
    workspace_requested_from_cmdline(&std::fs::read_to_string("/proc/cmdline")?)
}

fn workspace_requested_from_cmdline(cmdline: &str) -> anyhow::Result<bool> {
    Ok(cmdline
        .split_whitespace()
        .any(|token| token == WORKSPACE_CMDLINE_FLAG))
}

fn workspace_mkfs_allowed() -> anyhow::Result<bool> {
    workspace_mkfs_allowed_from_cmdline(&std::fs::read_to_string("/proc/cmdline")?)
}

fn workspace_mkfs_allowed_from_cmdline(cmdline: &str) -> anyhow::Result<bool> {
    Ok(cmdline
        .split_whitespace()
        .any(|token| token == WORKSPACE_MKFS_CMDLINE_FLAG))
}

fn run_workspace_command(program: &str, args: &[&str]) -> io::Result<()> {
    let status = Command::new(program).args(args).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{program} exited with status {status}"
        )))
    }
}

fn workspace_device_exists(device: &str) -> bool {
    Path::new(device).exists()
}

fn workspace_mount_ext4(device: &str, target: &str) -> io::Result<()> {
    mount_one(device, target, "ext4", MsFlags::empty()).map_err(io::Error::other)
}

fn workspace_repair_ext4(device: &str) -> io::Result<()> {
    run_workspace_command("e2fsck", &["-y", "-f", device])?;
    run_workspace_command("resize2fs", &[device])
}

fn workspace_mkfs_ext4(device: &str) -> io::Result<()> {
    run_workspace_command("mkfs.ext4", &["-F", device])
}

fn mount_workspace_device_if_present(
    boot_timer: &mut BootTimer,
    device: &str,
    target: &str,
    allow_mkfs_fallback: bool,
) -> anyhow::Result<()> {
    if !workspace_device_exists(device) {
        guest_log::info(
            GuestLogPhase::Boot,
            None,
            format!("no workspace drive ({device}), skipping workspace mount"),
        );
        boot_timer.mark("workspace_absent");
        return Ok(());
    }
    guest_log::info(
        GuestLogPhase::Boot,
        None,
        format!("step 11: mounting {device} at {target}"),
    );
    match workspace_mount_ext4(device, target) {
        Ok(()) => {
            boot_timer.mark("workspace_mounted");
            Ok(())
        }
        Err(initial) => {
            guest_log::warn(
                GuestLogPhase::Boot,
                None,
                format!(
                    "workspace mount failed: {initial}; running e2fsck/resize2fs on {device}"
                ),
            );
            workspace_repair_ext4(device)
                .with_context(|| format!("workspace repair failed for {device}"))?;
            boot_timer.mark("workspace_repaired");
            match workspace_mount_ext4(device, target) {
                Ok(()) => {
                    boot_timer.mark("workspace_mounted_after_repair");
                    Ok(())
                }
                Err(after_repair) if allow_mkfs_fallback => {
                    guest_log::warn(
                        GuestLogPhase::Boot,
                        None,
                        format!(
                            "workspace mount still failed after repair: {after_repair}; running mkfs.ext4 -F on {device}"
                        ),
                    );
                    workspace_mkfs_ext4(device)
                        .with_context(|| format!("workspace mkfs fallback failed for {device}"))?;
                    boot_timer.mark("workspace_reformatted");
                    workspace_mount_ext4(device, target).with_context(|| {
                        format!("workspace mount failed after mkfs fallback for {device}")
                    })?;
                    boot_timer.mark("workspace_mounted_after_mkfs");
                    Ok(())
                }
                Err(after_repair) => Err(anyhow::anyhow!(
                    "workspace mount failed after repair and mkfs fallback is disabled: initial={initial}; after_repair={after_repair}"
                )),
            }
        }
    }
}

// ── Test injection: trait + fake impl ────────────────────────────────────────

/// In tests, `WorkspaceMountOps` replaces the prod free functions for
/// controllable fakes. The prod path calls `workspace_*` free functions directly.
#[cfg(test)]
pub(crate) trait WorkspaceMountOps {
    fn device_exists(&self, device: &str) -> bool;
    fn mount_ext4(&self, device: &str, target: &str) -> io::Result<()>;
    fn repair_ext4(&self, device: &str) -> io::Result<()>;
    fn mkfs_ext4(&self, device: &str) -> io::Result<()>;
}

#[cfg(test)]
pub(crate) fn mount_workspace_device_with_ops(
    boot_timer: &mut BootTimer,
    device: &str,
    target: &str,
    allow_mkfs_fallback: bool,
    ops: &impl WorkspaceMountOps,
) -> anyhow::Result<()> {
    if !ops.device_exists(device) {
        guest_log::info(
            GuestLogPhase::Boot,
            None,
            format!("no workspace drive ({device}), skipping workspace mount"),
        );
        boot_timer.mark("workspace_absent");
        return Ok(());
    }
    guest_log::info(
        GuestLogPhase::Boot,
        None,
        format!("step 11: mounting {device} at {target}"),
    );
    match ops.mount_ext4(device, target) {
        Ok(()) => {
            boot_timer.mark("workspace_mounted");
            Ok(())
        }
        Err(initial) => {
            guest_log::warn(
                GuestLogPhase::Boot,
                None,
                format!(
                    "workspace mount failed: {initial}; running e2fsck/resize2fs on {device}"
                ),
            );
            ops.repair_ext4(device)
                .with_context(|| format!("workspace repair failed for {device}"))?;
            boot_timer.mark("workspace_repaired");
            match ops.mount_ext4(device, target) {
                Ok(()) => {
                    boot_timer.mark("workspace_mounted_after_repair");
                    Ok(())
                }
                Err(after_repair) if allow_mkfs_fallback => {
                    guest_log::warn(
                        GuestLogPhase::Boot,
                        None,
                        format!(
                            "workspace mount still failed after repair: {after_repair}; running mkfs.ext4 -F on {device}"
                        ),
                    );
                    ops.mkfs_ext4(device)
                        .with_context(|| format!("workspace mkfs fallback failed for {device}"))?;
                    boot_timer.mark("workspace_reformatted");
                    ops.mount_ext4(device, target).with_context(|| {
                        format!("workspace mount failed after mkfs fallback for {device}")
                    })?;
                    boot_timer.mark("workspace_mounted_after_mkfs");
                    Ok(())
                }
                Err(after_repair) => Err(anyhow::anyhow!(
                    "workspace mount failed after repair and mkfs fallback is disabled: initial={initial}; after_repair={after_repair}"
                )),
            }
        }
    }
}

/// Reap any pending zombies. Call between requests so orphans (children
/// of children that re-parent to PID 1) don't accumulate. Non-blocking;
/// returns once `waitpid(WNOHANG)` reports nothing more to reap.
pub(crate) fn reap_pending() {
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
        guest_log::error(
            GuestLogPhase::Boot,
            None,
            format!("m80-guestd PID-1 panic: {info}"),
        );
        std::process::exit(1);
    }));
}

#[cfg(test)]
mod tests;
