//! Public host-side hotplug request types.

use std::path::PathBuf;

/// Request to attach one preallocated Firecracker drive slot and verify the
/// guest-mounted identity bytes before returning the VM to the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotplugDriveAttach {
    /// Zero-based preallocated drive slot index.
    pub slot: u8,
    /// New Firecracker `path_on_host` for the slot. Because Firecracker is
    /// jailed, this path must be visible inside the jail namespace.
    pub path_on_host: PathBuf,
    /// Guest mount path, e.g. `/workspace`.
    pub mount_path: String,
    /// Guest file path to read after mount.
    pub identity_path: String,
    /// Opaque bytes expected from `identity_path`.
    pub expected_identity: Vec<u8>,
}

/// Request to ask the guest to unmount one preallocated drive slot and retarget
/// the Firecracker slot back to its placeholder backing file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotplugDriveDetach {
    /// Zero-based preallocated drive slot index.
    pub slot: u8,
    /// Guest mount path to unmount, e.g. `/workspace`.
    pub mount_path: String,
}
