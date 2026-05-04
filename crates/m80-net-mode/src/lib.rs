//! Resolver from caller intent (`NetworkPolicy`) to per-VM mode
//! (`VmNetworkMode`). See `README.md` for the contract.
//! Behavior captures: bead epic `m80-xbn`.

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
        /// Private-IPv4 destination CIDRs the VM may reach.
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
/// checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboundIntent {
    /// Bounded private-IPv4 CIDRs the VM may reach in addition to the
    /// admitted-DNS / public-IPv4 default-allow set.
    pub exceptions: Vec<Ipv4Net>,
    /// Optional override for the bridge gateway IP. Most callers leave this
    /// `None` and accept the deterministic derivation from the run-root path.
    pub gateway_override: Option<Ipv4Addr>,
}

/// Resolve caller intent to a per-VM network mode. Pure, no I/O. Infallible:
/// `exceptions` is already typed `Ipv4Net`, so CIDR parsing + IPv6 rejection
/// happen at the call-site before reaching here.
pub fn resolve(policy: &NetworkPolicy) -> VmNetworkMode {
    match policy {
        NetworkPolicy::NoEgress => VmNetworkMode::NoEgress,
        NetworkPolicy::AllowOutbound { exceptions } => VmNetworkMode::OutboundNat {
            plan: OutboundIntent {
                exceptions: exceptions.clone(),
                gateway_override: None,
            },
        },
    }
}
