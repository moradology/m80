//! Synchronous HTTP-over-UDS client for the Firecracker REST API.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: beads under `m80-t01.3` (`br show m80-t01.3`).

#![deny(missing_docs)]

use std::io;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

mod http;

/// A synchronous HTTP-over-UDS client for one Firecracker process.
///
/// One [`Client`] = one open socket. Concurrency is the caller's responsibility;
/// Firecracker itself does not handle concurrent config writes cleanly.
#[derive(Debug)]
pub struct Client {
    uds_path: PathBuf,
    stream: Mutex<UnixStream>,
}

impl Client {
    /// Open a connection to a Firecracker UDS API socket.
    pub fn new(uds_path: &Path) -> Result<Self, ClientError> {
        let stream = UnixStream::connect(uds_path).map_err(ClientError::Connect)?;
        Ok(Self {
            uds_path: uds_path.to_path_buf(),
            stream: Mutex::new(stream),
        })
    }

    /// PUT `/boot-source`.
    pub fn put_boot_source(&self, config: &BootSourceConfig) -> Result<(), ClientError> {
        let body = serde_json::to_vec(config)?;
        let resp = self.put("/boot-source", &body)?;
        if (200..300).contains(&resp.status) {
            return Ok(());
        }
        Err(ClientError::BootSourceWriteFailed {
            fault: body_to_string(&resp.body),
        })
    }

    /// PUT `/machine-config`.
    pub fn put_machine_config(&self, config: &MachineConfig) -> Result<(), ClientError> {
        let body = serde_json::to_vec(config)?;
        let resp = self.put("/machine-config", &body)?;
        if (200..300).contains(&resp.status) {
            return Ok(());
        }
        Err(ClientError::MachineConfigWriteFailed {
            fault: body_to_string(&resp.body),
        })
    }

    /// PUT `/drives/{drive_id}`. The `drive_id` is taken from the config.
    pub fn put_drive(&self, config: &DriveConfig) -> Result<(), ClientError> {
        let path = format!("/drives/{}", config.drive_id);
        let body = serde_json::to_vec(config)?;
        let resp = self.put(&path, &body)?;
        if (200..300).contains(&resp.status) {
            return Ok(());
        }
        Err(ClientError::DriveWriteFailed {
            fault: body_to_string(&resp.body),
        })
    }

    /// PUT `/network-interfaces/{iface_id}`.
    pub fn put_network_interface(&self, config: &NetworkInterfaceConfig) -> Result<(), ClientError> {
        let path = format!("/network-interfaces/{}", config.iface_id);
        let body = serde_json::to_vec(config)?;
        let resp = self.put(&path, &body)?;
        if (200..300).contains(&resp.status) {
            return Ok(());
        }
        Err(ClientError::NetworkInterfaceWriteFailed {
            fault: body_to_string(&resp.body),
        })
    }

    /// PUT `/vsock`.
    pub fn put_vsock(&self, config: &VsockConfig) -> Result<(), ClientError> {
        let body = serde_json::to_vec(config)?;
        let resp = self.put("/vsock", &body)?;
        if (200..300).contains(&resp.status) {
            return Ok(());
        }
        Err(ClientError::VsockWriteFailed {
            fault: body_to_string(&resp.body),
        })
    }

    /// PUT `/actions` with the requested action.
    pub fn instance_action(&self, action: InstanceAction) -> Result<(), ClientError> {
        // Serialize as `{"action_type": "PascalCaseVariant"}`.
        // `InstanceAction` uses `#[serde(rename_all = "PascalCase")]` so this
        // round-trip through `serde_json::Value` produces the right key/value.
        let payload = serde_json::json!({ "action_type": action });
        let body = serde_json::to_vec(&payload)?;
        let resp = self.put("/actions", &body)?;
        if (200..300).contains(&resp.status) {
            return Ok(());
        }
        Err(ClientError::InstanceActionFailed {
            action,
            fault: body_to_string(&resp.body),
        })
    }

    /// Send a PUT request over the stored `UnixStream`.
    ///
    /// A `Mutex` is used so `&self` methods can mutably access the stream.
    /// The caller is responsible for not calling concurrently — Firecracker
    /// itself does not handle concurrent config writes cleanly.
    fn put(&self, path: &str, body: &[u8]) -> Result<http::Response, ClientError> {
        let mut guard = self
            .stream
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // If the previous call left the stream in a broken state (e.g., the
        // firecracker process restarted), reconnect transparently.
        match http::put_json(&mut guard, path, body) {
            Ok(resp) => Ok(resp),
            Err(e) if is_broken_pipe(&e) => {
                // Reconnect once and retry.
                let new_stream =
                    UnixStream::connect(&self.uds_path).map_err(ClientError::Connect)?;
                *guard = new_stream;
                Ok(http::put_json(&mut guard, path, body)?)
            }
            Err(e) => Err(ClientError::Io(e)),
        }
    }
}

/// Return `true` for I/O errors that indicate the connection is dead and a
/// reconnect is worth attempting.
fn is_broken_pipe(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset
    )
}

/// Convert a response body to a `String`, falling back to lossy UTF-8.
fn body_to_string(body: &[u8]) -> String {
    String::from_utf8_lossy(body).into_owned()
}

// ---------------------------------------------------------------------------
// Firecracker config types
// ---------------------------------------------------------------------------

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

// `serde_json::Error` is not `io::Error`, so provide an explicit conversion
// so that serialization failures (which should not happen for our own types)
// surface cleanly rather than silently poisoning a `From<io::Error>` path.
impl From<serde_json::Error> for ClientError {
    fn from(e: serde_json::Error) -> Self {
        ClientError::Io(io::Error::new(io::ErrorKind::InvalidData, e))
    }
}
