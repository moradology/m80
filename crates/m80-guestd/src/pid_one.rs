//! PID-1 mode: when m80-guestd is the init process (no systemd).
//!
//! Activated only when `is_pid_one()` returns true. We avoid signal
//! handlers (which require `unsafe` and are async-signal-unsafe to write
//! correctly) — instead we poll-reap orphaned children between vsock
//! requests. The firecracker host's stop path is a SIGKILL on the
//! outside, so graceful SIGTERM handling buys nothing for v0.1.

use std::path::Path;

use anyhow::Context as _;
use nix::mount::{mount, MsFlags};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;

/// True when this process was launched by the kernel as init (PID 1).
pub fn is_pid_one() -> bool {
    std::process::id() == 1
}

/// Configure the process for PID-1 duty: pseudo-fs mounts, workspace
/// mount, and panic hook. Call exactly once early in `main`.
pub fn enter_pid_one_mode() -> anyhow::Result<()> {
    install_panic_hook();
    mount_pseudo_filesystems().context("pseudo-fs mounts")?;
    mount_workspace_if_present().context("workspace mount")?;
    Ok(())
}

/// Mount `/proc`, `/sys`, and `/dev`. The firecracker-ci kernel ships
/// `CONFIG_DEVTMPFS_MOUNT=y` so `/dev` is normally pre-populated by the
/// kernel — `EBUSY` (already mounted) is treated as success.
fn mount_pseudo_filesystems() -> anyhow::Result<()> {
    mount_one("proc",     "/proc", "proc",     MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_NOEXEC)?;
    mount_one("sysfs",    "/sys",  "sysfs",    MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_NOEXEC)?;
    mount_one("devtmpfs", "/dev",  "devtmpfs", MsFlags::MS_NOSUID)?;
    Ok(())
}

fn mount_one(source: &str, target: &str, fstype: &str, flags: MsFlags) -> anyhow::Result<()> {
    match mount(Some(source), Path::new(target), Some(fstype), flags, None::<&str>) {
        Ok(()) => Ok(()),
        Err(nix::errno::Errno::EBUSY) => Ok(()),
        Err(e) => Err(anyhow::anyhow!("mount {source} -> {target} ({fstype}): {e}")),
    }
}

/// Workspace device + mount point — must match the conventions used by
/// `m80-firecracker` (drive_id="workspace" → `/dev/vdb`) and by
/// `m80-image-build` (the rootfs pre-creates `/workspace`).
const WORKSPACE_DEV: &str = "/dev/vdb";
const WORKSPACE_TARGET: &str = "/workspace";

/// Mount the workspace drive at `/workspace` if attached. The workspace
/// is optional (a Sandbox without `workspace_dir` produces no
/// scratch.ext4 and no `/dev/vdb`); skip the mount in that case rather
/// than failing PID-1 setup. ENOENT on the device node is the documented
/// optional-state response, not silent error recovery.
fn mount_workspace_if_present() -> anyhow::Result<()> {
    if !Path::new(WORKSPACE_DEV).exists() {
        tracing::info!(
            dev = WORKSPACE_DEV,
            "no workspace drive attached, skipping workspace mount"
        );
        return Ok(());
    }
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
}
