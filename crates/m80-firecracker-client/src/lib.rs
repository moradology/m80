//! Synchronous HTTP-over-UDS client for the Firecracker REST API.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: beads under `m80-t01.3` (`br show m80-t01.3`).
//!
//! # Type-pinning pass
//!
//! Public surface is declared here; bodies are `todo!()`. Implementation lands
//! in a later wave.

#![deny(missing_docs)]

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A synchronous HTTP-over-UDS client for one Firecracker process.
///
/// One [`Client`] = one open socket. Concurrency is the caller's responsibility;
/// Firecracker itself does not handle concurrent config writes cleanly.
#[derive(Debug)]
pub struct Client {
    _uds_path: PathBuf,
}

impl Client {
    /// Open a connection to a Firecracker UDS API socket.
    pub fn new(_uds_path: &Path) -> Result<Self, ClientError> {
        todo!()
    }

    /// PUT `/boot-source`.
    pub fn put_boot_source(&self, _config: &BootSourceConfig) -> Result<(), ClientError> {
        todo!()
    }

    /// PUT `/machine-config`.
    pub fn put_machine_config(&self, _config: &MachineConfig) -> Result<(), ClientError> {
        todo!()
    }

    /// PUT `/drives/{drive_id}`. The `drive_id` is taken from the config.
    pub fn put_drive(&self, _config: &DriveConfig) -> Result<(), ClientError> {
        todo!()
    }

    /// PUT `/network-interfaces/{iface_id}`.
    pub fn put_network_interface(&self, _config: &NetworkInterfaceConfig) -> Result<(), ClientError> {
        todo!()
    }

    /// PUT `/vsock`.
    pub fn put_vsock(&self, _config: &VsockConfig) -> Result<(), ClientError> {
        todo!()
    }

    /// PUT `/actions` with the requested action.
    pub fn instance_action(&self, _action: InstanceAction) -> Result<(), ClientError> {
        todo!()
    }
}

/// `BootSource` config — kernel image path + boot args + optional initrd.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootSourceConfig {
    /// Absolute path to the kernel image inside the jail (or on the host
    /// when no jail is used).
    pub kernel_image_path: PathBuf,
    /// Boot args appended to the kernel command line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boot_args: Option<String>,
    /// Optional initrd path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initrd_path: Option<PathBuf>,
}

/// `MachineConfig` — vCPU count, memory size, SMT flag.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineConfig {
    /// Number of virtual CPUs.
    pub vcpu_count: u32,
    /// Memory size in MiB.
    pub mem_size_mib: u32,
    /// Symmetric multi-threading flag.
    #[serde(default)]
    pub smt: bool,
}

/// One drive slot. `drive_id == "rootfs"` is the conventional root drive id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveConfig {
    /// Stable drive identifier (e.g., `"rootfs"`, `"workspace"`).
    pub drive_id: String,
    /// Absolute path on the host (or jail) to the backing file.
    pub path_on_host: PathBuf,
    /// Whether this drive is the root device.
    pub is_root_device: bool,
    /// Whether this drive is mounted read-only.
    pub is_read_only: bool,
}

/// One network interface. `m80-firecracker` configures this only when the
/// resolved network mode is `OutboundNat`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterfaceConfig {
    /// Stable interface identifier.
    pub iface_id: String,
    /// Host-side tap device name.
    pub host_dev_name: String,
    /// Optional pinned guest MAC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guest_mac: Option<String>,
}

/// Vsock device config — guest CID + host UDS path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VsockConfig {
    /// Vsock context id assigned to the guest.
    pub guest_cid: u32,
    /// Host UDS path that bridges to the guest CID.
    pub uds_path: PathBuf,
}

/// Lifecycle action requested via PUT `/actions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum InstanceAction {
    /// Boot the VM.
    InstanceStart,
    /// Send Ctrl-Alt-Del — graceful stop on x86_64; unsupported elsewhere.
    SendCtrlAltDel,
    /// Flush metrics buffers to the configured fifo.
    FlushMetrics,
    /// Pause vCPU execution.
    Pause,
    /// Resume vCPU execution.
    Resume,
}

/// Errors surfaced by the client. Each variant names the resource that failed
/// so the orchestrator can attach phase context.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// The UDS could not be opened.
    #[error("connect: {0}")]
    Connect(io::Error),
    /// `PUT /boot-source` failed.
    #[error("boot-source write failed: {fault}")]
    BootSourceWriteFailed {
        /// Firecracker fault JSON (verbatim).
        fault: String,
    },
    /// `PUT /machine-config` failed.
    #[error("machine-config write failed: {fault}")]
    MachineConfigWriteFailed {
        /// Firecracker fault JSON (verbatim).
        fault: String,
    },
    /// `PUT /drives/{id}` failed.
    #[error("drive write failed: {fault}")]
    DriveWriteFailed {
        /// Firecracker fault JSON (verbatim).
        fault: String,
    },
    /// `PUT /network-interfaces/{id}` failed.
    #[error("network-interface write failed: {fault}")]
    NetworkInterfaceWriteFailed {
        /// Firecracker fault JSON (verbatim).
        fault: String,
    },
    /// `PUT /vsock` failed.
    #[error("vsock write failed: {fault}")]
    VsockWriteFailed {
        /// Firecracker fault JSON (verbatim).
        fault: String,
    },
    /// `PUT /actions` failed.
    #[error("instance action {action:?} failed: {fault}")]
    InstanceActionFailed {
        /// The action that was attempted.
        action: InstanceAction,
        /// Firecracker fault JSON (verbatim).
        fault: String,
    },
    /// Underlying I/O failure.
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
}
