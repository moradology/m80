//! Synchronous HTTP-over-UDS client for the Firecracker REST API.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: beads under `m80-t01.3` (`br show m80-t01.3`).

#![deny(missing_docs)]

mod debug_wire;
mod http;

use std::io;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

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
        if ok(resp.status) {
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
        if ok(resp.status) {
            return Ok(());
        }
        Err(ClientError::MachineConfigWriteFailed {
            fault: body_to_string(&resp.body),
        })
    }

    /// PUT `/logger`.
    pub fn put_logger(&self, config: &LoggerConfig) -> Result<(), ClientError> {
        let body = serde_json::to_vec(config)?;
        let resp = self.put("/logger", &body)?;
        if ok(resp.status) {
            return Ok(());
        }
        Err(ClientError::LoggerWriteFailed {
            fault: body_to_string(&resp.body),
        })
    }

    /// PUT `/metrics`.
    pub fn put_metrics(&self, config: &MetricsConfig) -> Result<(), ClientError> {
        let body = serde_json::to_vec(config)?;
        let resp = self.put("/metrics", &body)?;
        if ok(resp.status) {
            return Ok(());
        }
        Err(ClientError::MetricsWriteFailed {
            fault: body_to_string(&resp.body),
        })
    }

    /// PUT `/drives/{drive_id}`. The `drive_id` is taken from the config.
    pub fn put_drive(&self, config: &DriveConfig) -> Result<(), ClientError> {
        let path = format!("/drives/{}", config.drive_id);
        let body = serde_json::to_vec(config)?;
        let resp = self.put(&path, &body)?;
        if ok(resp.status) {
            return Ok(());
        }
        Err(ClientError::DriveWriteFailed {
            fault: body_to_string(&resp.body),
        })
    }

    /// PUT `/pmem/{id}`. The `id` is taken from the config.
    pub fn put_pmem(&self, config: &PmemConfig) -> Result<(), ClientError> {
        let path = format!("/pmem/{}", config.id);
        let body = serde_json::to_vec(config)?;
        let resp = self.put(&path, &body)?;
        if ok(resp.status) {
            return Ok(());
        }
        Err(ClientError::PmemWriteFailed {
            fault: body_to_string(&resp.body),
        })
    }

    /// PATCH `/drives/{drive_id}`. The `drive_id` is taken from the config.
    pub fn patch_drive(&self, config: &PartialDriveConfig) -> Result<(), ClientError> {
        let path = format!("/drives/{}", config.drive_id);
        let body = serde_json::to_vec(config)?;
        let resp = self.patch(&path, &body)?;
        if ok(resp.status) {
            return Ok(());
        }
        Err(ClientError::DriveWriteFailed {
            fault: body_to_string(&resp.body),
        })
    }

    /// PUT `/vsock`.
    pub fn put_vsock(&self, config: &VsockConfig) -> Result<(), ClientError> {
        let body = serde_json::to_vec(config)?;
        let resp = self.put("/vsock", &body)?;
        if ok(resp.status) {
            return Ok(());
        }
        Err(ClientError::VsockWriteFailed {
            fault: body_to_string(&resp.body),
        })
    }

    /// PUT `/entropy` — add one virtio-rng entropy device.
    pub fn put_entropy_device(&self) -> Result<(), ClientError> {
        let body = b"{}";
        let resp = self.put("/entropy", body)?;
        if ok(resp.status) {
            return Ok(());
        }
        Err(ClientError::EntropyDeviceWriteFailed {
            fault: body_to_string(&resp.body),
        })
    }

    /// PUT `/network-interfaces/{iface_id}`. The `iface_id` is taken from the config.
    pub fn put_network_interface(
        &self,
        config: &NetworkInterfaceConfig,
    ) -> Result<(), ClientError> {
        let path = format!("/network-interfaces/{}", config.iface_id);
        let body = serde_json::to_vec(config)?;
        let resp = self.put(&path, &body)?;
        if ok(resp.status) {
            return Ok(());
        }
        Err(ClientError::NetworkInterfaceWriteFailed {
            fault: body_to_string(&resp.body),
        })
    }

    /// PATCH `/vm` — set the VM running state (`Paused` or `Resumed`).
    pub fn patch_vm_state(&self, state: VmState) -> Result<(), ClientError> {
        let body = serde_json::to_vec(&VmStatePayload { state })?;
        let resp = self.patch("/vm", &body)?;
        if ok(resp.status) {
            return Ok(());
        }
        Err(ClientError::VmStateWriteFailed {
            state,
            fault: body_to_string(&resp.body),
        })
    }

    /// PUT `/snapshot/create` — write a snapshot of the paused microVM to disk.
    ///
    /// The VM **must** be paused before calling this (via
    /// `patch_vm_state(VmState::Paused)`). Firecracker will return an error if
    /// the VM is still running.
    pub fn put_snapshot_create(&self, config: &CreateSnapshotConfig) -> Result<(), ClientError> {
        let body = serde_json::to_vec(config)?;
        let resp = self.put("/snapshot/create", &body)?;
        if ok(resp.status) {
            return Ok(());
        }
        Err(ClientError::SnapshotCreateFailed {
            fault: body_to_string(&resp.body),
        })
    }

    /// PUT `/snapshot/load` — restore a microVM from a snapshot file pair.
    ///
    /// Must be called **before** the VM has been started (pre-boot). The
    /// `vsock.sock` file from any previous VM using the same jail must be
    /// removed before calling this — Firecracker rebinds the UDS at load time
    /// and will fail with `EADDRINUSE` if the file already exists.
    pub fn put_snapshot_load(&self, config: &LoadSnapshotConfig) -> Result<(), ClientError> {
        let body = serde_json::to_vec(config)?;
        let resp = self.put("/snapshot/load", &body)?;
        if ok(resp.status) {
            return Ok(());
        }
        Err(ClientError::SnapshotLoadFailed {
            fault: body_to_string(&resp.body),
        })
    }

    /// GET `/version` — read the Firecracker binary version.
    pub fn get_version(&self) -> Result<FirecrackerVersion, ClientError> {
        let resp = self.get("/version")?;
        if !ok(resp.status) {
            return Err(ClientError::VersionReadFailed {
                fault: body_to_string(&resp.body),
            });
        }
        serde_json::from_slice(&resp.body).map_err(|source| ClientError::VersionReadFailed {
            fault: source.to_string(),
        })
    }

    /// PUT `/actions` with the requested action.
    pub fn instance_action(&self, action: InstanceAction) -> Result<(), ClientError> {
        let body = serde_json::to_vec(&InstanceActionPayload {
            action_type: action,
        })?;
        let resp = self.put("/actions", &body)?;
        if ok(resp.status) {
            return Ok(());
        }
        Err(ClientError::InstanceActionFailed {
            action,
            fault: body_to_string(&resp.body),
        })
    }

    /// Send a PATCH request over the stored `UnixStream`.
    fn patch(&self, path: &str, body: &[u8]) -> Result<http::Response, ClientError> {
        self.send("PATCH", path, body)
    }

    /// Send a GET request over the stored `UnixStream`.
    fn get(&self, path: &str) -> Result<http::Response, ClientError> {
        self.send("GET", path, b"")
    }

    /// Send a PUT request over the stored `UnixStream`.
    fn put(&self, path: &str, body: &[u8]) -> Result<http::Response, ClientError> {
        self.send("PUT", path, body)
    }

    /// Send one JSON request over the stored `UnixStream`.
    ///
    /// A `Mutex` is used so `&self` methods can mutably access the stream.
    /// The caller is responsible for not calling concurrently — Firecracker
    /// itself does not handle concurrent config writes cleanly.
    fn send(
        &self,
        method: &'static str,
        path: &str,
        body: &[u8],
    ) -> Result<http::Response, ClientError> {
        trace_request(method, path, body);
        let mut guard = self
            .stream
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // If the previous call left the stream in a broken state (e.g., the
        // firecracker process restarted), reconnect transparently.
        let resp = match http::send_json(&mut *guard, method, path, body) {
            Ok(resp) => resp,
            Err(e) if is_broken_pipe(&e) => {
                // Reconnect once and retry.
                let new_stream =
                    UnixStream::connect(&self.uds_path).map_err(ClientError::Connect)?;
                *guard = new_stream;
                http::send_json(&mut *guard, method, path, body).map_err(|e| ClientError::Io {
                    path: self.uds_path.clone(),
                    source: e,
                })?
            }
            Err(e) => {
                return Err(ClientError::Io {
                    path: self.uds_path.clone(),
                    source: e,
                })
            }
        };
        trace_response(&resp);
        Ok(resp)
    }
}

/// Emit a `tracing::trace!` for an outgoing request when `fcrest` wire logging is enabled.
fn trace_request(method: &str, path: &str, body: &[u8]) {
    if debug_wire::is_enabled("fcrest") {
        tracing::trace!(
            direction = "out",
            method,
            path,
            preview = %debug_wire::format_wire_preview(body),
            "fcrest request"
        );
    }
}

/// Emit a `tracing::trace!` for an incoming response when `fcrest` wire logging is enabled.
fn trace_response(resp: &http::Response) {
    if debug_wire::is_enabled("fcrest") {
        tracing::trace!(
            direction = "in",
            status = resp.status,
            preview = %debug_wire::format_wire_preview(&resp.body),
            "fcrest response"
        );
    }
}

/// Return `true` when `status` is a 2xx success.
fn ok(status: u16) -> bool {
    (200..300).contains(&status)
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
///
/// `String::from_utf8` avoids the extra allocation on the (typical) valid-UTF-8 path.
fn body_to_string(body: &[u8]) -> String {
    String::from_utf8(body.to_vec())
        .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

// ---------------------------------------------------------------------------
// Firecracker config types
// ---------------------------------------------------------------------------
//
// These schema structs stay public even when m80-firecracker is the only
// current production consumer: this crate's contract is a generic
// Firecracker REST speaker, and its public methods intentionally mirror the
// Firecracker request bodies.

/// `BootSource` config — kernel image path + boot args + optional initrd.
#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
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

/// Firecracker CPU template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CpuTemplate {
    /// AWS T2-compatible template.
    T2,
    /// AWS C3-compatible template.
    C3,
}

/// `MachineConfig` — vCPU count, memory size, SMT flag, CPU template, and
/// optional dirty-page tracking.
#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MachineConfig {
    /// Number of virtual CPUs.
    pub vcpu_count: u32,
    /// Memory size in MiB.
    pub mem_size_mib: u32,
    /// Symmetric multi-threading flag.
    #[serde(default)]
    pub smt: bool,
    /// CPU template. m80 sets this for a stable, narrowed guest CPU surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_template: Option<CpuTemplate>,
    /// Enable Firecracker dirty-page tracking for future diff snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_dirty_pages: Option<bool>,
}

/// Firecracker logger verbosity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum LogLevel {
    /// Error events only.
    Error,
    /// Warnings and errors.
    Warning,
    /// Informational events, warnings, and errors.
    Info,
    /// Debug, informational, warning, and error events.
    Debug,
}

/// Firecracker structured logger config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LoggerConfig {
    /// Absolute path visible to the Firecracker process for its log output.
    pub log_path: PathBuf,
    /// Optional logger verbosity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<LogLevel>,
    /// Include the level name in each record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_level: Option<bool>,
    /// Include the Firecracker source module in each record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_log_origin: Option<bool>,
}

/// Firecracker metrics output config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MetricsConfig {
    /// Absolute path visible to the Firecracker process for JSON metrics.
    pub metrics_path: PathBuf,
}

/// Firecracker block-device I/O engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum IoEngine {
    /// Synchronous file I/O.
    Sync,
    /// io_uring-backed asynchronous file I/O.
    Async,
}

/// Firecracker block-device host cache policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum CacheType {
    /// Conservative host writeback cache with durability barriers.
    Writeback,
    /// Skip host-side sync/barrier work. Suitable only for ephemeral writes.
    Unsafe,
}

/// One drive slot. `drive_id == "rootfs"` is the conventional root drive id.
#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DriveConfig {
    /// Stable drive identifier (e.g., `"rootfs"`, `"workspace"`).
    pub drive_id: String,
    /// Absolute path on the host (or jail) to the backing file.
    pub path_on_host: PathBuf,
    /// Whether this drive is the root device.
    pub is_root_device: bool,
    /// Whether this drive is mounted read-only.
    pub is_read_only: bool,
    /// Optional Firecracker block-device I/O engine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub io_engine: Option<IoEngine>,
    /// Optional Firecracker block-device host cache policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_type: Option<CacheType>,
}

/// Post-boot update for an existing drive slot.
#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PartialDriveConfig {
    /// Stable drive identifier (e.g., `"workspace_slot_0"`).
    pub drive_id: String,
    /// New absolute host path for the existing drive backing file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_on_host: Option<PathBuf>,
}

/// Firecracker persistent-memory device config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PmemConfig {
    /// Stable pmem identifier, used in the `/pmem/{id}` URL.
    pub id: String,
    /// Absolute path on the host (or jail) to the pmem backing file.
    pub path_on_host: PathBuf,
    /// Whether this pmem device is the root device.
    #[serde(default)]
    pub root_device: bool,
    /// Whether this pmem backing is read-only.
    #[serde(default)]
    pub read_only: bool,
}

/// Vsock device config — guest CID + host UDS path.
#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VsockConfig {
    /// Vsock context id assigned to the guest.
    pub guest_cid: u32,
    /// Host UDS path that bridges to the guest CID.
    pub uds_path: PathBuf,
}

/// Virtio-net device config — host TAP name plus guest-visible identity.
#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkInterfaceConfig {
    /// Stable network interface identifier in Firecracker.
    pub iface_id: String,
    /// Host TAP device name Firecracker attaches to the guest.
    pub host_dev_name: String,
    /// Optional guest MAC address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guest_mac: Option<String>,
    /// Optional receive-side rate limiter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rx_rate_limiter: Option<RateLimiterConfig>,
    /// Optional transmit-side rate limiter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_rate_limiter: Option<RateLimiterConfig>,
}

/// Firecracker token-bucket rate limiter config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TokenBucketConfig {
    /// Total number of tokens the bucket can hold.
    pub size: u64,
    /// Optional initial burst budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub one_time_burst: Option<u64>,
    /// Milliseconds required to refill the bucket.
    pub refill_time: u64,
}

/// Firecracker I/O rate limiter config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RateLimiterConfig {
    /// Optional bytes-per-refill token bucket.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bandwidth: Option<TokenBucketConfig>,
    /// Optional operations-per-refill token bucket.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ops: Option<TokenBucketConfig>,
}

/// VM running state — used with PATCH `/vm`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum VmState {
    /// Pause vCPU execution (required before snapshot creation).
    Paused,
    /// Resume vCPU execution.
    Resumed,
}

/// Snapshot type: full copy of all guest memory, or diff since the last snapshot.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SnapshotType {
    /// Full snapshot — all guest memory pages are saved.
    Full,
    /// Diff snapshot — only pages dirtied since the previous snapshot are saved.
    Diff,
}

/// Parameters for `PUT /snapshot/create`.
///
/// The VM must be paused before calling this endpoint. On success, Firecracker
/// writes two files: the microVM state file (`snapshot_path`) and the guest
/// memory file (`mem_file_path`). Both paths must be writable by the
/// Firecracker process.
#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateSnapshotConfig {
    /// Path to write the microVM state file (device + vCPU register state).
    pub snapshot_path: PathBuf,
    /// Path to write the guest memory file.
    pub mem_file_path: PathBuf,
    /// Snapshot type. Defaults to `Full` when omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_type: Option<SnapshotType>,
}

/// Memory backend type for snapshot load.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MemBackendType {
    /// Load memory from a regular file (`mmap(MAP_PRIVATE)`).
    File,
    /// Load memory via userfaultfd — caller provides page-fault handler.
    Uffd,
}

/// Memory backend configuration for `PUT /snapshot/load`.
#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemBackendConfig {
    /// How guest memory is loaded on restore.
    pub backend_type: MemBackendType,
    /// Path to the memory file (for `File`) or UFFD socket (for `Uffd`).
    pub backend_path: PathBuf,
}

/// Vsock override — redirect the vsock UDS path on restore.
///
/// Use this when restoring into a jail with a different path than the one
/// embedded in the snapshot, or when restoring multiple VMs from the same
/// snapshot (each needs its own UDS path).
#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VsockOverride {
    /// New host UDS path for the vsock device.
    pub uds_path: PathBuf,
}

/// Parameters for `PUT /snapshot/load`.
///
/// The vsock UDS file from any previous VM using the same jail path must be
/// removed before calling this — Firecracker rebinds the socket at load time
/// and fails with `EADDRINUSE` if the file already exists. Use
/// `vsock_override` to redirect to a different path when needed.
///
/// Exactly one of `mem_backend` or `mem_file_path` must be present.
#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LoadSnapshotConfig {
    /// Path to the microVM state file produced by `PUT /snapshot/create`.
    pub snapshot_path: PathBuf,
    /// Memory backend configuration (preferred; mutually exclusive with `mem_file_path`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem_backend: Option<MemBackendConfig>,
    /// Path to the guest memory file (deprecated; use `mem_backend` instead).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem_file_path: Option<PathBuf>,
    /// Enable dirty page tracking for future diff snapshots.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_diff_snapshots: Option<bool>,
    /// When `true`, resume the VM immediately after a successful load.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_vm: Option<bool>,
    /// Override the vsock UDS path embedded in the snapshot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vsock_override: Option<VsockOverride>,
}

/// Firecracker version response returned by GET `/version`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FirecrackerVersion {
    /// Firecracker build version string.
    pub firecracker_version: String,
}

/// Lifecycle action requested via PUT `/actions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum InstanceAction {
    /// Boot the VM.
    InstanceStart,
}

#[derive(Serialize)]
struct VmStatePayload {
    state: VmState,
}

#[derive(Serialize)]
struct InstanceActionPayload {
    action_type: InstanceAction,
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
    /// `PUT /logger` failed.
    #[error("logger write failed: {fault}")]
    LoggerWriteFailed {
        /// Firecracker fault JSON (verbatim).
        fault: String,
    },
    /// `PUT /metrics` failed.
    #[error("metrics write failed: {fault}")]
    MetricsWriteFailed {
        /// Firecracker fault JSON (verbatim).
        fault: String,
    },
    /// `PUT` or `PATCH` `/drives/{id}` failed.
    #[error("drive write failed: {fault}")]
    DriveWriteFailed {
        /// Firecracker fault JSON (verbatim).
        fault: String,
    },
    /// `PUT /pmem/{id}` failed.
    #[error("pmem write failed: {fault}")]
    PmemWriteFailed {
        /// Firecracker fault JSON (verbatim).
        fault: String,
    },
    /// `PUT /vsock` failed.
    #[error("vsock write failed: {fault}")]
    VsockWriteFailed {
        /// Firecracker fault JSON (verbatim).
        fault: String,
    },
    /// `PUT /entropy` failed.
    #[error("entropy device write failed: {fault}")]
    EntropyDeviceWriteFailed {
        /// Firecracker fault JSON (verbatim).
        fault: String,
    },
    /// `PUT /network-interfaces/{id}` failed.
    #[error("network interface write failed: {fault}")]
    NetworkInterfaceWriteFailed {
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
    /// `PATCH /vm` (pause/resume) failed.
    #[error("vm state {state:?} write failed: {fault}")]
    VmStateWriteFailed {
        /// The state transition that was attempted.
        state: VmState,
        /// Firecracker fault JSON (verbatim).
        fault: String,
    },
    /// `PUT /snapshot/create` failed.
    #[error("snapshot create failed: {fault}")]
    SnapshotCreateFailed {
        /// Firecracker fault JSON (verbatim).
        fault: String,
    },
    /// `PUT /snapshot/load` failed.
    #[error("snapshot load failed: {fault}")]
    SnapshotLoadFailed {
        /// Firecracker fault JSON (verbatim).
        fault: String,
    },
    /// `GET /version` failed or returned an invalid response body.
    #[error("version read failed: {fault}")]
    VersionReadFailed {
        /// Firecracker fault JSON or parse error.
        fault: String,
    },
    /// Request-body serialization failed before any HTTP request was sent.
    #[error("serialize request body: {0}")]
    Serialize(#[from] serde_json::Error),
    /// Underlying I/O failure; carries the socket path so callers don't have
    /// to guess which UDS operation failed.
    #[error("i/o on {}: {source}", path.display())]
    Io {
        /// Socket path the I/O was attempted against.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
}
