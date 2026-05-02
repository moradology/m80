//! Resolver from caller intent to `VmNetworkMode` (NoEgress | OutboundNat).
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epic `m80-xbn` (`br show m80-xbn`).
//!
//! # Type-pinning pass
//!
//! Public surface is declared here; bodies are `todo!()`. Implementation lands
//! in a later wave.

#![deny(missing_docs)]

use std::net::Ipv4Addr;

use ipnet::Ipv4Net;
use serde::{Deserialize, Serialize};

/// Caller intent: opt-in network egress with optional bounded private
/// exceptions. Default for any new construction is [`NetworkPolicy::NoEgress`].
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NetworkPolicy {
    /// No NIC, no iptables, no privilege required.
    #[default]
    NoEgress,
    /// Outbound NAT to admitted public-IPv4 destinations, plus bounded
    /// private-IPv4 exception CIDRs.
    AllowOutbound {
        /// Pre-validated private-IPv4 destination CIDRs the VM may reach.
        exceptions: Vec<Ipv4Net>,
    },
}

/// Resolved per-VM network mode. `m80-firecracker` consumes only this; it
/// never inspects the original `NetworkPolicy`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VmNetworkMode {
    /// VM gets no NIC; iptables untouched.
    NoEgress,
    /// VM gets a tap on the run-root bridge; rules per [`OutboundIntent`].
    OutboundNat {
        /// Pre-validated payload for `m80-net-outbound` to realize.
        plan: OutboundIntent,
    },
}

/// Pre-validated payload for `m80-net-outbound::realize`. The resolver hands
/// this off; the realizer does not re-validate fields the resolver already
/// checked (Ipv6 rejection, syntactic CIDR parsing).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboundIntent {
    /// Bounded private-IPv4 CIDRs the VM may reach in addition to the
    /// admitted-DNS / public-IPv4 default-allow set.
    pub exceptions: Vec<Ipv4Net>,
    /// Optional override for the bridge gateway IP. Most callers leave this
    /// `None` and accept the deterministic derivation from the run-root path.
    pub gateway_override: Option<Ipv4Addr>,
}

/// Errors surfaced by [`resolve`].
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    /// An entry in `exceptions` was IPv6. v0.1 is IPv4-only.
    #[error("IPv6 is not supported in v0.1")]
    Ipv6Unsupported,
    /// A CIDR string failed to parse.
    #[error("malformed CIDR: {0}")]
    MalformedCidr(String),
}

/// Resolve caller intent to a per-VM network mode. Pure: no I/O.
pub fn resolve(_policy: &NetworkPolicy) -> Result<VmNetworkMode, ResolveError> {
    todo!()
}
