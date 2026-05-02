//! Integration tests for [`m80_net_mode::resolve`].
//!
//! Beads covered:
//!   m80-xbn.2.1 — single-seam resolver (bool collapse)
//!   m80-xbn.2.2 — `resolve` is the only entry point for mode decisions

use ipnet::Ipv4Net;
use m80_net_mode::{NetworkPolicy, OutboundIntent, VmNetworkMode, resolve};

/// Bead m80-xbn.2.1, m80-xbn.2.2: `NetworkPolicy::default()` is `NoEgress`.
/// The default-deny invariant lives in one place and can be pinned here.
#[test]
fn default_network_policy_is_noegress() {
    assert_eq!(NetworkPolicy::default(), NetworkPolicy::NoEgress);
}

/// Bead m80-xbn.1.1, m80-xbn.1.2, m80-xbn.1.3:
/// `NoEgress` policy resolves to `VmNetworkMode::NoEgress` — no NIC, no
/// iptables, no bridge. The resolver is the single seam that encodes this.
#[test]
fn noegress_resolves_to_noegress() {
    assert_eq!(resolve(&NetworkPolicy::NoEgress), VmNetworkMode::NoEgress);
}

/// `AllowOutbound` with an empty exception list resolves to `OutboundNat`
/// with an empty `exceptions` vec and no gateway override.
#[test]
fn allow_outbound_with_no_exceptions_resolves_to_outbound_nat() {
    let mode = resolve(&NetworkPolicy::AllowOutbound { exceptions: vec![] });
    assert_eq!(
        mode,
        VmNetworkMode::OutboundNat {
            plan: OutboundIntent {
                exceptions: vec![],
                gateway_override: None,
            }
        }
    );
}

/// `AllowOutbound` with a concrete CIDR propagates that CIDR unchanged into
/// the `OutboundIntent`, and `gateway_override` is `None`.
#[test]
fn allow_outbound_with_one_cidr_round_trips_through_resolver() {
    let cidr: Ipv4Net = "10.0.0.0/8".parse().unwrap();
    let policy = NetworkPolicy::AllowOutbound {
        exceptions: vec![cidr],
    };
    let mode = resolve(&policy);
    assert_eq!(
        mode,
        VmNetworkMode::OutboundNat {
            plan: OutboundIntent {
                exceptions: vec!["10.0.0.0/8".parse().unwrap()],
                gateway_override: None,
            }
        }
    );
}

/// `NetworkPolicy` round-trips through `serde_json` for both variants.
#[test]
fn network_policy_roundtrips_through_serde_json_noegress() {
    let policy = NetworkPolicy::NoEgress;
    let json = serde_json::to_string(&policy).unwrap();
    let back: NetworkPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(back, policy);
}

/// `NetworkPolicy::AllowOutbound` with a CIDR round-trips through `serde_json`.
#[test]
fn network_policy_roundtrips_through_serde_json_allow_outbound() {
    let policy = NetworkPolicy::AllowOutbound {
        exceptions: vec!["192.168.1.0/24".parse().unwrap()],
    };
    let json = serde_json::to_string(&policy).unwrap();
    let back: NetworkPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(back, policy);
}

/// `VmNetworkMode::NoEgress` round-trips through `serde_json`.
#[test]
fn vm_network_mode_roundtrips_through_serde_json_noegress() {
    let mode = VmNetworkMode::NoEgress;
    let json = serde_json::to_string(&mode).unwrap();
    let back: VmNetworkMode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, mode);
}

/// `VmNetworkMode::OutboundNat` round-trips through `serde_json`.
#[test]
fn vm_network_mode_roundtrips_through_serde_json_outbound_nat() {
    let mode = VmNetworkMode::OutboundNat {
        plan: OutboundIntent {
            exceptions: vec!["172.16.0.0/12".parse().unwrap()],
            gateway_override: None,
        },
    };
    let json = serde_json::to_string(&mode).unwrap();
    let back: VmNetworkMode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, mode);
}
