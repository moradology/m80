//! OutboundNat: per-host bridge, per-VM tap/IP/MAC, default-deny iptables,
//! ownership-aware cleanup. See `README.md` for the contract.
//! Behavior captures: bead epic `m80-exy`.

#![deny(missing_docs)]

mod dns;
mod injection;
mod iptables;
mod link_ops;
mod state;
mod teardown;

use std::fs;
use std::io;
use std::net::Ipv4Addr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use ipnet::Ipv4Net;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use dns::{
    discover_dns_resolvers, discover_dns_resolvers_with_ops, is_admitted_dns_resolver,
    DnsCommandOutput, DnsDiscoveryOps,
};
pub use injection::{
    build_guest_network_config, inject_guest_network_config, inject_guest_network_config_with_ops,
    GuestNetworkConfig, GuestNetworkConfigOps, M80_NETWORKD_FILE, M80_RESOLVED_FILE,
    SYSTEMD_NETWORK_DIR, SYSTEMD_RESOLVED_CONF_DIR,
};
pub use iptables::{
    apply_outbound_nat_policy, apply_outbound_nat_policy_with_ops, outbound_nat_filter_chain,
    outbound_nat_rule_comment, permanent_deny_cidrs, PolicyCommandOutput, PolicyOps,
};
pub use link_ops::LinkOps;
pub use m80_net_mode::OutboundIntent;
pub use state::{
    bridge_state_path, planned_bridge_state, planned_vm_network_state, read_bridge_state,
    read_vm_network_state_record, vm_network_state_path, write_bridge_state,
    write_vm_network_state_record, BridgeState, SetupPhase, VmNetworkStateRecord,
    BRIDGE_STATE_FILE, NETWORK_STATE_SCHEMA_VERSION,
};
pub use teardown::{
    cleanup_orphan_bridge, cleanup_orphan_bridge_with_ops, cleanup_outbound_nat_policy_with_ops,
    cleanup_vm, cleanup_vm_with_ops,
};

/// Comment prefix m80 stamps on every iptables rule it owns. Used by cleanup
/// to find rules by comment match (never by index).
pub const M80_RULE_COMMENT_PREFIX: &str = "m80";

/// Per-VM network state filename under each VM run directory.
pub const NETWORK_STATE_FILE: &str = "network-state.json";

/// What the realizer materialized for one VM. Caller-observable handles for
/// diagnostics and downstream config (e.g., the orchestrator's
/// `NetworkInterfaceConfig` build).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RealizedNetwork {
    /// Bridge interface name (e.g., `brfc<11hex>`).
    pub bridge_name: String,
    /// Tap interface name (e.g., `tfc<12hex>`).
    pub tap_name: String,
    /// Guest IPv4 address (deterministic per `(run_root, vm_id)`).
    pub guest_ipv4: Ipv4Addr,
    /// Guest MAC address (locally-administered, deterministic).
    pub guest_mac: String,
    /// Bridge CIDR (a /24 inside `172.16.0.0/12`).
    pub bridge_cidr: Ipv4Net,
}

/// Realize `intent` for `vm_id` under `run_root`: address allocation,
/// collision detection, bridge + tap setup, guest network injection,
/// iptables policy installation. The returned [`RealizedNetwork`] is what
/// the orchestrator hands to `m80-firecracker-client` to PUT.
pub fn realize(
    _intent: &OutboundIntent,
    _vm_id: &str,
    _run_root: &Path,
) -> Result<RealizedNetwork, NetError> {
    todo!()
}

/// Realize only bridge/TAP setup with the real host link backend.
///
/// This is the Phase 3/4 boundary used before guest network injection and
/// iptables policy are wired into top-level [`realize`]. Callers must pass
/// existing `run_root` and `run_dir` directories; this function writes state
/// atomically but does not create parent directories.
pub fn realize_bridge_and_tap(
    intent: &OutboundIntent,
    vm_id: &str,
    run_root: &Path,
    run_dir: &Path,
) -> Result<RealizedNetwork, NetError> {
    let mut ops = link_ops::NetlinkLinkOps::new()?;
    realize_bridge_and_tap_with_ops(&mut ops, intent, vm_id, run_root, run_dir)
}

/// Realize bridge/TAP setup through a supplied link-ops backend.
///
/// This function is public so integration tests and future callers can pin the
/// state-machine contract without requiring `CAP_NET_ADMIN`.
pub fn realize_bridge_and_tap_with_ops(
    ops: &mut impl LinkOps,
    intent: &OutboundIntent,
    vm_id: &str,
    run_root: &Path,
    run_dir: &Path,
) -> Result<RealizedNetwork, NetError> {
    let bridge = planned_bridge_state(run_root, intent)?;
    ensure_bridge_ready_with_ops(ops, run_root, &bridge)?;

    let vm_state = planned_vm_network_state(intent, vm_id, run_root, run_dir, bridge.clone());
    write_vm_network_state_record(run_dir, &vm_state)?;

    let tap_plan = link_ops::TapBridgePlan {
        bridge_name: bridge.bridge_name.clone(),
        tap_name: vm_state.tap_name.clone(),
        bridge_cidr: bridge.cidr,
        guest_mac: derive_guest_mac_bytes(run_root, vm_id),
    };
    link_ops::create_tap_on_bridge(ops, &tap_plan)?;

    let ready_vm_state = vm_state.with_phase(SetupPhase::Ready);
    write_vm_network_state_record(run_dir, &ready_vm_state)?;

    Ok(RealizedNetwork {
        bridge_name: bridge.bridge_name,
        tap_name: ready_vm_state.tap_name,
        guest_ipv4: ready_vm_state.guest_ipv4,
        guest_mac: ready_vm_state.guest_mac,
        bridge_cidr: ready_vm_state.bridge.cidr,
    })
}

/// Derive the bridge name from a run-root path. Pure: no I/O.
///
/// Format: `brfc` followed by the first 11 hex chars of `sha256(run_root_path)`.
pub fn derive_bridge_name(run_root: &Path) -> String {
    let digest = run_root_digest(run_root);
    format!("brfc{}", &hex::encode(digest)[..11])
}

/// Derive the tap name from a run-root path and VM id. Pure: no I/O.
///
/// Format: `tfc` followed by the first 12 hex chars of
/// `sha256(run_root_path || vm_id)`.
pub fn derive_tap_name(run_root: &Path, vm_id: &str) -> String {
    let digest = vm_digest(run_root, vm_id);
    format!("tfc{}", &hex::encode(digest)[..12])
}

/// Derive the guest IPv4 + MAC for a VM. Pure: no I/O.
pub fn derive_guest_addressing(run_root: &Path, vm_id: &str) -> (Ipv4Addr, String) {
    let bridge_cidr = derive_bridge_cidr(run_root);
    let digest = vm_digest(run_root, vm_id);
    let host = 2 + (((u16::from(digest[5]) << 8) | u16::from(digest[6])) % 253) as u8;
    let [a, b, c, _] = bridge_cidr.network().octets();
    let guest_ipv4 = Ipv4Addr::new(a, b, c, host);
    let guest_mac = format!(
        "02:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        digest[0], digest[1], digest[2], digest[3], digest[4]
    );
    (guest_ipv4, guest_mac)
}

/// Derive the bridge CIDR for a run-root. Pure: no I/O.
///
/// Format: `172.<o2>.<o3>.0/24` where `o2 = (sha256(run_root)[0] % 16) + 16`
/// and `o3 = sha256(run_root)[1]`.
pub fn derive_bridge_cidr(run_root: &Path) -> Ipv4Net {
    let digest = run_root_digest(run_root);
    let o2 = (digest[0] % 16) + 16;
    let o3 = digest[1];
    Ipv4Net::new(Ipv4Addr::new(172, o2, o3, 0), 24).expect("static /24 prefix is valid")
}

/// Reject a planned guest IPv4 if a sibling VM state file already claims it.
pub fn reject_guest_ipv4_collision(
    run_root: &Path,
    current_vm_id: &str,
    planned_bridge_cidr: Ipv4Net,
    planned_guest_ipv4: Ipv4Addr,
) -> Result<(), NetError> {
    if !run_root.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(run_root)? {
        let entry = entry?;
        let candidate_dir = entry.path();
        if !candidate_dir.is_dir() {
            continue;
        }

        let state_path = candidate_dir.join(NETWORK_STATE_FILE);
        if !state_path.exists() {
            continue;
        }
        let state = read_vm_network_state(&state_path)?;
        if state.vm_id == current_vm_id {
            continue;
        }
        if state.bridge.cidr == planned_bridge_cidr && state.guest_ipv4 == planned_guest_ipv4 {
            return Err(NetError::GuestIpv4Collision {
                peer_vm_id: state.vm_id,
            });
        }
    }

    Ok(())
}

/// Reject a planned bridge CIDR if it overlaps a non-default host route.
pub fn reject_host_route_collision(
    planned_cidr: Ipv4Net,
    allowed_interface: Option<&str>,
) -> Result<(), NetError> {
    let text = fs::read_to_string("/proc/net/route")?;
    reject_host_route_collision_from_proc_net_route(planned_cidr, allowed_interface, &text)
}

/// Reject a planned bridge CIDR against supplied `/proc/net/route` text.
pub fn reject_host_route_collision_from_proc_net_route(
    planned_cidr: Ipv4Net,
    allowed_interface: Option<&str>,
    route_text: &str,
) -> Result<(), NetError> {
    for route in parse_proc_net_route(route_text, Path::new("/proc/net/route"))? {
        if route.cidr.prefix_len() == 0 {
            continue;
        }
        if allowed_interface
            .is_some_and(|interface| route.interface == interface && route.cidr == planned_cidr)
        {
            continue;
        }
        if ipv4_net_overlaps(planned_cidr, route.cidr) {
            return Err(NetError::HostRouteCollision {
                existing: format!("{} {}", route.interface, route.cidr),
            });
        }
    }
    Ok(())
}

fn run_root_digest(run_root: &Path) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(run_root.as_os_str().as_bytes());
    hasher.finalize().into()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VmNetworkState {
    vm_id: String,
    bridge: VmBridgeState,
    guest_ipv4: Ipv4Addr,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VmBridgeState {
    cidr: Ipv4Net,
}

#[derive(Debug)]
struct HostRoute {
    interface: String,
    cidr: Ipv4Net,
}

fn read_vm_network_state(path: &Path) -> Result<VmNetworkState, NetError> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(|source| NetError::InvalidNetworkState {
        path: path.to_path_buf(),
        detail: source.to_string(),
    })
}

fn parse_proc_net_route(text: &str, path: &Path) -> Result<Vec<HostRoute>, NetError> {
    let mut routes = Vec::new();
    for (line_number, line) in text.lines().enumerate() {
        if line_number == 0 || line.trim().is_empty() {
            continue;
        }
        let columns = line.split_whitespace().collect::<Vec<_>>();
        if columns.len() < 8 {
            return Err(NetError::InvalidNetworkState {
                path: path.to_path_buf(),
                detail: format!("route line {} has too few columns", line_number + 1),
            });
        }
        let destination = parse_proc_route_ipv4(columns[1], path)?;
        let mask = parse_proc_route_ipv4(columns[7], path)?;
        let prefix_len =
            ipv4_mask_prefix_len(mask).ok_or_else(|| NetError::InvalidNetworkState {
                path: path.to_path_buf(),
                detail: format!(
                    "route line {} has non-contiguous mask {mask}",
                    line_number + 1
                ),
            })?;
        let cidr = Ipv4Net::new(destination, prefix_len).map_err(|source| {
            NetError::InvalidNetworkState {
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

fn parse_proc_route_ipv4(hex: &str, path: &Path) -> Result<Ipv4Addr, NetError> {
    let raw = u32::from_str_radix(hex, 16).map_err(|_| NetError::InvalidNetworkState {
        path: path.to_path_buf(),
        detail: format!("invalid procfs IPv4 hex value {hex:?}"),
    })?;
    Ok(Ipv4Addr::new(
        (raw & 0xff) as u8,
        ((raw >> 8) & 0xff) as u8,
        ((raw >> 16) & 0xff) as u8,
        ((raw >> 24) & 0xff) as u8,
    ))
}

fn ipv4_mask_prefix_len(mask: Ipv4Addr) -> Option<u8> {
    let mask = u32::from(mask);
    let prefix_len = mask.count_ones() as u8;
    let expected = if prefix_len == 0 {
        0
    } else {
        u32::MAX << (32 - prefix_len)
    };
    (mask == expected).then_some(prefix_len)
}

fn ipv4_net_overlaps(a: Ipv4Net, b: Ipv4Net) -> bool {
    a.contains(&b.network()) || b.contains(&a.network())
}

fn vm_digest(run_root: &Path, vm_id: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(run_root.as_os_str().as_bytes());
    hasher.update(vm_id.as_bytes());
    hasher.finalize().into()
}

fn derive_guest_mac_bytes(run_root: &Path, vm_id: &str) -> [u8; 6] {
    let digest = vm_digest(run_root, vm_id);
    [0x02, digest[0], digest[1], digest[2], digest[3], digest[4]]
}

fn ensure_bridge_ready_with_ops(
    ops: &mut impl LinkOps,
    run_root: &Path,
    planned: &BridgeState,
) -> Result<(), NetError> {
    let state_path = bridge_state_path(run_root);
    if state_path.exists() {
        let existing = read_bridge_state(run_root)?;
        if !state::bridge_state_matches_identity(&existing, planned) {
            return Err(NetError::BridgeOwnershipMismatch);
        }
        if existing.setup_phase == SetupPhase::Ready {
            if ops.link_has_ipv4_address(
                &planned.bridge_name,
                planned.gateway_ipv4,
                planned.cidr.prefix_len(),
            )? {
                return Ok(());
            }
            return Err(NetError::InvalidNetworkState {
                path: state_path,
                detail: format!(
                    "ready bridge state exists but {} lacks {}/{}",
                    planned.bridge_name,
                    planned.gateway_ipv4,
                    planned.cidr.prefix_len()
                ),
            });
        }
    }

    write_bridge_state(run_root, planned)?;
    ops.create_bridge(&planned.bridge_name)?;
    ops.add_ipv4_address(
        &planned.bridge_name,
        planned.gateway_ipv4,
        planned.cidr.prefix_len(),
    )?;
    ops.set_link_up(&planned.bridge_name)?;
    write_bridge_state(run_root, &planned.clone().with_phase(SetupPhase::Ready))
}

/// Errors surfaced by network operations.
#[derive(Debug, thiserror::Error)]
pub enum NetError {
    /// IPv6 was requested but is not supported in v0.1.
    #[error("IPv6 is not supported in v0.1")]
    Ipv6Unsupported,
    /// Another VM in the run-root already claims this guest IPv4.
    #[error("guest IPv4 collision with peer VM {peer_vm_id}")]
    GuestIpv4Collision {
        /// VM id that already holds the IP.
        peer_vm_id: String,
    },
    /// The derived bridge CIDR overlaps an existing host route.
    #[error("host route collision: {existing}")]
    HostRouteCollision {
        /// Description of the conflicting host route.
        existing: String,
    },
    /// A network state file or procfs route entry was malformed.
    #[error("invalid network state at {}: {detail}", path.display())]
    InvalidNetworkState {
        /// File that contained invalid network state.
        path: PathBuf,
        /// Description of the invalid field or parse failure.
        detail: String,
    },
    /// On-disk ownership state did not match the derived bridge identity.
    #[error("bridge ownership mismatch")]
    BridgeOwnershipMismatch,
    /// A host network command returned a non-zero exit.
    #[error("{program} failed: {stderr}")]
    NetworkCommandFailed {
        /// Program that failed.
        program: String,
        /// Captured stderr (best-effort UTF-8).
        stderr: String,
    },
    /// No usable public IPv4 DNS resolver was discovered.
    #[error("no usable DNS resolvers")]
    NoUsableDnsResolvers,
    /// An rtnetlink operation failed.
    #[error("rtnetlink operation {operation} failed: {detail}")]
    NetlinkOperationFailed {
        /// Operation being attempted.
        operation: &'static str,
        /// Error detail returned by the netlink stack.
        detail: String,
    },
    /// TAP creation through the kernel TUN/TAP driver failed.
    #[error("tap operation {operation} failed: {source}")]
    TapOperationFailed {
        /// Operation being attempted.
        operation: &'static str,
        /// I/O error returned by the TAP backend.
        #[source]
        source: io::Error,
    },
    /// A link needed for an operation was not present.
    #[error("link {name} not found while trying to {operation}")]
    LinkNotFound {
        /// Operation being attempted.
        operation: &'static str,
        /// Interface name that could not be found.
        name: String,
    },
    /// A pre-existing rule in our owned chain was found.
    #[error("foreign rule in owned chain: {rule}")]
    ForeignChainRule {
        /// The offending rule text.
        rule: String,
    },
    /// Network allocation or cleanup found owned-resource residue it cannot safely handle.
    #[error("network allocation conflict at {}: {detail}", path.display())]
    NetworkAllocationConflict {
        /// Path-like owner for the conflicting resource.
        path: PathBuf,
        /// Description of the conflict.
        detail: String,
    },
    /// Underlying I/O failure.
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
}
