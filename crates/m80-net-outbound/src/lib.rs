//! OutboundNat: per-host bridge, per-VM tap/IP/MAC, default-deny iptables,
//! ownership-aware cleanup.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epic `m80-exy` (`br show m80-exy`).
//!
//! # Type-pinning pass
//!
//! Public surface is declared here; bodies are `todo!()`. Implementation lands
//! in a later wave.

#![deny(missing_docs)]

use std::io;
use std::net::Ipv4Addr;
use std::path::Path;

use ipnet::Ipv4Net;
use serde::{Deserialize, Serialize};

pub use m80_net_mode::OutboundIntent;

/// Comment prefix m80 stamps on every iptables rule it owns. Used by cleanup
/// to find rules by comment match (never by index).
pub const RULE_COMMENT_PREFIX: &str = "m80:";

/// What the realizer materialized for one VM. Caller-observable handles for
/// diagnostics and downstream config (e.g., the orchestrator's
/// `NetworkInterfaceConfig` build).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealizedNetwork {
    /// Bridge interface name (e.g., `brfc<12hex>`).
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

/// Tear down the network state owned by `vm_id`. Idempotent and ownership-
/// aware: only rules with our comment prefix and taps we created are removed.
pub fn cleanup_vm(_vm_id: &str, _run_root: &Path) -> Result<(), NetError> {
    todo!()
}

/// Remove the run-root bridge if no peer VM in the run-root references it.
/// Called at startup. Tolerant of malformed/missing state files.
pub fn cleanup_orphan_bridge(_run_root: &Path) -> Result<(), NetError> {
    todo!()
}

/// Derive the bridge name from a run-root path. Pure: no I/O.
///
/// Format: `brfc` followed by the first 12 hex chars of `sha256(run_root_path)`.
pub fn derive_bridge_name(_run_root: &Path) -> String {
    todo!()
}

/// Derive the tap name from a run-root path and VM id. Pure: no I/O.
///
/// Format: `tfc` followed by the first 12 hex chars of
/// `sha256(run_root_path || vm_id)`.
pub fn derive_tap_name(_run_root: &Path, _vm_id: &str) -> String {
    todo!()
}

/// Derive the guest IPv4 + MAC for a VM. Pure: no I/O.
pub fn derive_guest_addressing(_run_root: &Path, _vm_id: &str) -> (Ipv4Addr, String) {
    todo!()
}

/// Derive the bridge CIDR for a run-root. Pure: no I/O.
///
/// Format: `172.<o2>.<o3>.0/24` where `o2 = (sha256(run_root)[0] % 16) + 16`
/// and `o3 = sha256(run_root)[1]`.
pub fn derive_bridge_cidr(_run_root: &Path) -> Ipv4Net {
    todo!()
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
    /// On-disk ownership state did not match the derived bridge identity.
    #[error("bridge ownership mismatch")]
    BridgeOwnershipMismatch,
    /// `iptables` returned a non-zero exit.
    #[error("iptables failed: {stderr}")]
    IptablesCommandFailed {
        /// Captured stderr (best-effort UTF-8).
        stderr: String,
    },
    /// `ip` returned a non-zero exit.
    #[error("ip failed: {stderr}")]
    IpCommandFailed {
        /// Captured stderr (best-effort UTF-8).
        stderr: String,
    },
    /// A pre-existing rule in our owned chain was found.
    #[error("foreign rule in owned chain: {rule}")]
    ForeignChainRule {
        /// The offending rule text.
        rule: String,
    },
    /// Underlying I/O failure.
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
}
