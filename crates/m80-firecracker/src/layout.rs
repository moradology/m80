//! Pure path helpers for the per-VM run-root layout.

use std::path::{Path, PathBuf};

/// Firecracker REST API socket filename inside the jail root.
pub const FIRECRACKER_API_SOCKET: &str = "firecracker.sock";

/// Host-side vsock muxer socket filename inside the jail root.
pub const VSOCK_SOCKET: &str = "vsock.sock";

/// Per-VM writable root filesystem overlay filename in the run directory.
pub const ROOTFS_OVERLAY_IMAGE: &str = "rootfs.overlay.ext4";

/// Per-VM writable workspace scratch image filename in the run directory.
pub const SCRATCH_IMAGE: &str = "scratch.ext4";

pub(crate) const PREALLOCATED_DRIVE_SLOT_PREFIX: &str = "hotplug-slot-";

/// Per-VM console and process stderr log filename in the run directory.
pub const CONSOLE_LOG: &str = "console.log";

/// Boot artifact identity filename in the run directory.
pub const BOOT_IDENTITY_FILE: &str = "boot-identity.json";

/// Compute the per-VM run directory under the backend run root.
pub fn run_dir_path(run_root: &Path, vm_id: &str) -> PathBuf {
    run_root.join(vm_id)
}

/// Compute the Firecracker REST API socket path visible to the host.
///
/// The socket is created inside the jailer root, not directly in the run
/// directory.
pub fn firecracker_api_socket_path(run_dir: &Path, firecracker_bin: &Path) -> PathBuf {
    m80_jailer::jail_root_path(run_dir, firecracker_bin).join(FIRECRACKER_API_SOCKET)
}

/// Compute the host-side vsock muxer socket path.
///
/// The socket is created inside the jailer root, not directly in the run
/// directory.
pub fn vsock_socket_path(run_dir: &Path, firecracker_bin: &Path) -> PathBuf {
    m80_jailer::jail_root_path(run_dir, firecracker_bin).join(VSOCK_SOCKET)
}

/// Compute the per-VM writable root filesystem overlay path.
pub fn rootfs_overlay_path(run_dir: &Path) -> PathBuf {
    run_dir.join(ROOTFS_OVERLAY_IMAGE)
}

/// Compute the per-VM writable workspace scratch image path.
pub fn scratch_image_path(run_dir: &Path) -> PathBuf {
    run_dir.join(SCRATCH_IMAGE)
}

pub(crate) fn preallocated_drive_slot_filename(slot: u8) -> String {
    format!("{PREALLOCATED_DRIVE_SLOT_PREFIX}{slot}.raw")
}

pub(crate) fn preallocated_drive_slot_path(run_dir: &Path, slot: u8) -> PathBuf {
    run_dir.join(preallocated_drive_slot_filename(slot))
}

pub(crate) fn preallocated_drive_slot_jail_path(slot: u8) -> PathBuf {
    PathBuf::from(format!("/{}", preallocated_drive_slot_filename(slot)))
}

/// Compute the console log path for the VM.
pub fn console_log_path(run_dir: &Path) -> PathBuf {
    run_dir.join(CONSOLE_LOG)
}

/// Compute the boot identity record path for the VM.
pub fn boot_identity_path(run_dir: &Path) -> PathBuf {
    run_dir.join(BOOT_IDENTITY_FILE)
}
