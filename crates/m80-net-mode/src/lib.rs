//! Resolver from caller intent (`NetworkPolicy`) to per-VM mode
//! (`VmNetworkMode`). See `README.md` for the contract.
//! Behavior captures: bead epic `m80-xbn`.

#![deny(missing_docs)]

use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::str::FromStr;

use ipnet::Ipv4Net;
use serde::{Deserialize, Serialize};

/// Caller intent: opt-in network egress with optional bounded private
/// exceptions. Callers must pick a variant explicitly; `NoEgress` is the
/// conservative choice when in doubt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NetworkPolicy {
    /// No NIC, no iptables, no privilege required.
    NoEgress,
    /// Outbound NAT to admitted public-IPv4 destinations, plus bounded
    /// private-IPv4 exception CIDRs.
    AllowOutbound {
        /// Private-IPv4 destination CIDRs the VM may reach.
        exceptions: Vec<Ipv4Net>,
    },
    /// Join an externally provisioned network namespace. The caller owns the
    /// namespace's interfaces, routing, firewall policy, and lifetime.
    JoinNetns {
        /// Caller-owned namespace and static guest network contract.
        spec: NetnsSpec,
    },
}

/// Caller-owned network namespace and static guest networking contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetnsSpec {
    /// Path to a namespace fd, usually `/var/run/netns/<name>`.
    pub netns_path: PathBuf,
    /// Caller-created TAP device visible inside `netns_path`.
    pub tap_name: String,
    /// Guest MAC address assigned to the Firecracker virtio-net device.
    pub guest_mac: MacAddr,
    /// Guest IPv4 address with prefix configured by PID 1.
    pub guest_ipv4: Ipv4Net,
    /// Default gateway configured by PID 1.
    pub gateway_ipv4: Ipv4Addr,
    /// DNS resolvers written into the guest by PID 1.
    pub dns_resolvers: Vec<Ipv4Addr>,
}

/// Colon-separated Ethernet MAC address accepted by Firecracker and m80's
/// PID-1 network cmdline.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct MacAddr(String);

impl MacAddr {
    /// Parse a strict six-octet colon-separated MAC address.
    pub fn parse(value: impl Into<String>) -> Result<Self, MacAddrParseError> {
        let value = value.into();
        if is_valid_mac_addr(&value) {
            Ok(Self(value))
        } else {
            Err(MacAddrParseError::InvalidFormat { value })
        }
    }
}

impl std::fmt::Display for MacAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for MacAddr {
    type Err = MacAddrParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl<'de> Deserialize<'de> for MacAddr {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

/// Rejection reason for an invalid [`MacAddr`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacAddrParseError {
    /// Value is not `HH:HH:HH:HH:HH:HH` with hexadecimal octets.
    InvalidFormat {
        /// Rejected input.
        value: String,
    },
}

impl std::fmt::Display for MacAddrParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MacAddrParseError::InvalidFormat { value } => {
                write!(
                    f,
                    "invalid MAC address {value:?}: expected HH:HH:HH:HH:HH:HH"
                )
            }
        }
    }
}

impl std::error::Error for MacAddrParseError {}

fn is_valid_mac_addr(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 17 {
        return false;
    }
    for (index, byte) in bytes.iter().enumerate() {
        if index % 3 == 2 {
            if *byte != b':' {
                return false;
            }
        } else if !byte.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

/// Resolved per-VM network mode. `m80-firecracker` consumes only this; it
/// never inspects the original `NetworkPolicy`.
#[derive(Debug, PartialEq, Eq)]
pub enum VmNetworkMode {
    /// VM gets no NIC; iptables untouched.
    NoEgress,
    /// VM gets a tap on the run-root bridge; rules per [`OutboundIntent`].
    OutboundNat {
        /// Pre-validated payload for `m80-net-outbound` to realize.
        plan: OutboundIntent,
    },
    /// Launch Firecracker after joining a caller-provided network namespace.
    JoinNetns {
        /// Caller-owned namespace and static guest network contract.
        spec: NetnsSpec,
    },
}

/// Pre-validated payload for `m80-net-outbound::realize`. The resolver hands
/// this off; the realizer does not re-validate fields the resolver already
/// checked.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutboundIntent {
    /// Bounded private-IPv4 CIDRs the VM may reach in addition to the
    /// admitted-DNS / public-IPv4 default-allow set.
    pub exceptions: Vec<Ipv4Net>,
}

/// Resolve caller intent to a per-VM network mode. Pure, no I/O. Infallible:
/// `exceptions` is already typed `Ipv4Net`, so CIDR parsing + IPv6 rejection
/// happen at the call-site before reaching here.
#[must_use]
pub fn resolve(policy: &NetworkPolicy) -> VmNetworkMode {
    match policy {
        NetworkPolicy::NoEgress => VmNetworkMode::NoEgress,
        NetworkPolicy::AllowOutbound { exceptions } => VmNetworkMode::OutboundNat {
            plan: OutboundIntent {
                exceptions: exceptions.clone(),
            },
        },
        NetworkPolicy::JoinNetns { spec } => VmNetworkMode::JoinNetns { spec: spec.clone() },
    }
}
