use std::fs;
use std::io::Write;
use std::net::Ipv4Addr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ipnet::Ipv4Net;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    derive_bridge_cidr, derive_guest_addressing, derive_tap_name, NetError, OutboundIntent,
};

/// Run-root-level bridge ownership state filename.
pub const BRIDGE_STATE_FILE: &str = "outbound-bridge-state.json";

/// Schema version for bridge and per-VM network state files.
pub const NETWORK_STATE_SCHEMA_VERSION: u32 = 1;

/// Setup phase recorded in bridge and VM network state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum SetupPhase {
    /// State has been planned and written before host link mutation.
    Planned,
    /// Host link mutation succeeded and the state is ready for consumers.
    Ready,
}

/// Run-root-level ownership record for the outbound bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BridgeState {
    /// State schema version.
    pub schema_version: u32,
    /// Current setup phase.
    pub setup_phase: SetupPhase,
    /// Run-root path that owns the bridge.
    pub run_root: PathBuf,
    /// Hex-encoded SHA-256 digest of the run-root path.
    pub run_root_digest: String,
    /// Derived Linux bridge interface name.
    pub bridge_name: String,
    /// Derived bridge CIDR.
    pub cidr: Ipv4Net,
    /// IPv4 gateway assigned to the bridge.
    pub gateway_ipv4: Ipv4Addr,
}

impl BridgeState {
    /// Return this state with a different setup phase.
    pub fn with_phase(mut self, setup_phase: SetupPhase) -> Self {
        self.setup_phase = setup_phase;
        self
    }
}

/// Per-VM network state written after bridge/TAP setup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VmNetworkStateRecord {
    /// State schema version.
    pub schema_version: u32,
    /// Current setup phase.
    pub setup_phase: SetupPhase,
    /// VM id this state belongs to.
    pub vm_id: String,
    /// Per-VM run directory.
    pub run_dir: PathBuf,
    /// Embedded run-root bridge state.
    pub bridge: BridgeState,
    /// Derived host TAP interface name.
    pub tap_name: String,
    /// Guest MAC address.
    pub guest_mac: String,
    /// Guest IPv4 address.
    pub guest_ipv4: Ipv4Addr,
    /// Private IPv4 exception CIDRs admitted by caller policy.
    pub private_ipv4_exceptions: Vec<Ipv4Net>,
    /// DNS resolvers discovered for guest injection.
    pub dns_resolvers: Vec<Ipv4Addr>,
    /// Whether guest rootfs network configuration has been written.
    pub runtime_rootfs_configured: bool,
}

impl VmNetworkStateRecord {
    /// Return this state with a different setup phase.
    pub fn with_phase(mut self, setup_phase: SetupPhase) -> Self {
        self.setup_phase = setup_phase;
        self
    }
}

/// Return the path to the run-root bridge state file.
pub fn bridge_state_path(run_root: &Path) -> PathBuf {
    run_root.join(BRIDGE_STATE_FILE)
}

/// Return the path to the per-VM network state file.
pub fn vm_network_state_path(run_dir: &Path) -> PathBuf {
    run_dir.join(crate::NETWORK_STATE_FILE)
}

/// Build the planned bridge state for a run-root and outbound intent.
pub fn planned_bridge_state(
    run_root: &Path,
    intent: &OutboundIntent,
) -> Result<BridgeState, NetError> {
    let cidr = derive_bridge_cidr(run_root);
    let gateway_ipv4 = match intent.gateway_override {
        Some(gateway) if cidr.hosts().any(|host| host == gateway) => gateway,
        Some(gateway) => {
            return Err(NetError::InvalidNetworkState {
                path: run_root.to_path_buf(),
                detail: format!("gateway override {gateway} is not a usable host in {cidr}"),
            });
        }
        None => cidr
            .hosts()
            .next()
            .ok_or_else(|| NetError::InvalidNetworkState {
                path: run_root.to_path_buf(),
                detail: format!("{cidr} has no usable gateway host"),
            })?,
    };

    Ok(BridgeState {
        schema_version: NETWORK_STATE_SCHEMA_VERSION,
        setup_phase: SetupPhase::Planned,
        run_root: run_root.to_path_buf(),
        run_root_digest: run_root_digest_hex(run_root),
        bridge_name: crate::derive_bridge_name(run_root),
        cidr,
        gateway_ipv4,
    })
}

/// Build the planned per-VM network state for a VM.
pub fn planned_vm_network_state(
    intent: &OutboundIntent,
    vm_id: &str,
    run_root: &Path,
    run_dir: &Path,
    bridge: BridgeState,
) -> VmNetworkStateRecord {
    let (guest_ipv4, guest_mac) = derive_guest_addressing(run_root, vm_id);
    VmNetworkStateRecord {
        schema_version: NETWORK_STATE_SCHEMA_VERSION,
        setup_phase: SetupPhase::Planned,
        vm_id: vm_id.to_owned(),
        run_dir: run_dir.to_path_buf(),
        bridge,
        tap_name: derive_tap_name(run_root, vm_id),
        guest_mac,
        guest_ipv4,
        private_ipv4_exceptions: intent.exceptions.clone(),
        dns_resolvers: Vec::new(),
        runtime_rootfs_configured: false,
    }
}

/// Read the run-root bridge state file.
pub fn read_bridge_state(run_root: &Path) -> Result<BridgeState, NetError> {
    read_json_state(&bridge_state_path(run_root))
}

/// Atomically write the run-root bridge state file.
pub fn write_bridge_state(run_root: &Path, state: &BridgeState) -> Result<(), NetError> {
    write_json_atomically(&bridge_state_path(run_root), state)
}

/// Read a per-VM network state file.
pub fn read_vm_network_state_record(run_dir: &Path) -> Result<VmNetworkStateRecord, NetError> {
    read_json_state(&vm_network_state_path(run_dir))
}

/// Atomically write a per-VM network state file.
pub fn write_vm_network_state_record(
    run_dir: &Path,
    state: &VmNetworkStateRecord,
) -> Result<(), NetError> {
    write_json_atomically(&vm_network_state_path(run_dir), state)
}

/// Minimal projection of a VM network state for collision detection.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VmNetworkStateMinimal {
    pub(crate) vm_id: String,
    pub(crate) bridge: VmBridgeStateMinimal,
    pub(crate) guest_ipv4: std::net::Ipv4Addr,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VmBridgeStateMinimal {
    pub(crate) cidr: Ipv4Net,
}

pub(crate) struct HostRoute {
    pub(crate) interface: String,
    pub(crate) cidr: Ipv4Net,
}

pub(crate) fn read_vm_network_state_minimal(
    path: &Path,
) -> Result<VmNetworkStateMinimal, crate::NetError> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(|source| crate::NetError::InvalidNetworkState {
        path: path.to_path_buf(),
        detail: source.to_string(),
    })
}

pub(crate) fn parse_host_routes(
    text: &str,
    path: &Path,
) -> Result<Vec<HostRoute>, crate::NetError> {
    let mut routes = Vec::new();
    for (line_number, line) in text.lines().enumerate() {
        if line_number == 0 || line.trim().is_empty() {
            continue;
        }
        let columns = line.split_whitespace().collect::<Vec<_>>();
        if columns.len() < 8 {
            return Err(crate::NetError::InvalidNetworkState {
                path: path.to_path_buf(),
                detail: format!("route line {} has too few columns", line_number + 1),
            });
        }
        let destination = parse_proc_route_ipv4(columns[1], path)?;
        let mask = parse_proc_route_ipv4(columns[7], path)?;
        let prefix_len =
            ipv4_mask_prefix_len(mask).ok_or_else(|| crate::NetError::InvalidNetworkState {
                path: path.to_path_buf(),
                detail: format!(
                    "route line {} has non-contiguous mask {mask}",
                    line_number + 1
                ),
            })?;
        let cidr = Ipv4Net::new(destination, prefix_len).map_err(|source| {
            crate::NetError::InvalidNetworkState {
                path: path.to_path_buf(),
                detail: source.to_string(),
            }
        })?;
        routes.push(HostRoute {
            interface: columns[0].to_owned(),
            cidr,
        });
    }
    Ok(routes)
}

fn parse_proc_route_ipv4(hex: &str, path: &Path) -> Result<std::net::Ipv4Addr, crate::NetError> {
    let raw = u32::from_str_radix(hex, 16).map_err(|_| crate::NetError::InvalidNetworkState {
        path: path.to_path_buf(),
        detail: format!("invalid procfs IPv4 hex value {hex:?}"),
    })?;
    Ok(std::net::Ipv4Addr::new(
        (raw & 0xff) as u8,
        ((raw >> 8) & 0xff) as u8,
        ((raw >> 16) & 0xff) as u8,
        ((raw >> 24) & 0xff) as u8,
    ))
}

fn ipv4_mask_prefix_len(mask: std::net::Ipv4Addr) -> Option<u8> {
    let mask = u32::from(mask);
    let prefix_len = mask.count_ones() as u8;
    let expected = if prefix_len == 0 {
        0
    } else {
        u32::MAX << (32 - prefix_len)
    };
    (mask == expected).then_some(prefix_len)
}

pub(crate) fn bridge_state_matches_identity(existing: &BridgeState, planned: &BridgeState) -> bool {
    existing.schema_version == planned.schema_version
        && existing.run_root == planned.run_root
        && existing.run_root_digest == planned.run_root_digest
        && existing.bridge_name == planned.bridge_name
        && existing.cidr == planned.cidr
        && existing.gateway_ipv4 == planned.gateway_ipv4
}

pub(crate) fn run_root_digest_hex(run_root: &Path) -> String {
    hex::encode(run_root_digest(run_root))
}

pub(crate) fn run_root_digest(run_root: &Path) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(run_root.as_os_str().as_bytes());
    hasher.finalize().into()
}

fn read_json_state<T>(path: &Path) -> Result<T, NetError>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(|source| NetError::InvalidNetworkState {
        path: path.to_path_buf(),
        detail: source.to_string(),
    })
}

fn write_json_atomically<T>(path: &Path, value: &T) -> Result<(), NetError>
where
    T: Serialize,
{
    let parent = path.parent().ok_or_else(|| NetError::InvalidNetworkState {
        path: path.to_path_buf(),
        detail: "state path has no parent directory".to_owned(),
    })?;
    let mut bytes =
        serde_json::to_vec_pretty(value).map_err(|source| NetError::InvalidNetworkState {
            path: path.to_path_buf(),
            detail: source.to_string(),
        })?;
    bytes.push(b'\n');

    let tmp = unique_tmp_path(path);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn unique_tmp_path(path: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state");
    path.with_file_name(format!(".{file_name}.{}.{}.tmp", std::process::id(), nanos))
}
