//! Integration tests for [`m80_net_mode::resolve`].
//!
//! Beads: m80-xbn.1.* (resolver semantics), m80-xbn.2.* (single-seam decision).

use ipnet::Ipv4Net;
use m80_net_mode::{resolve, NetworkPolicy, OutboundIntent, VmNetworkMode};

#[test]
fn noegress_resolves_to_noegress() {
    assert_eq!(resolve(&NetworkPolicy::NoEgress), VmNetworkMode::NoEgress);
}

#[test]
fn allow_outbound_with_no_exceptions_resolves_to_outbound_nat() {
    let mode = resolve(&NetworkPolicy::AllowOutbound { exceptions: vec![] });
    assert_eq!(
        mode,
        VmNetworkMode::OutboundNat {
            plan: OutboundIntent {
                exceptions: vec![],
                gateway_override: None
            },
        }
    );
}

#[test]
fn allow_outbound_with_one_cidr_round_trips_through_resolver() {
    let cidr: Ipv4Net = "10.0.0.0/8".parse().unwrap();
    let mode = resolve(&NetworkPolicy::AllowOutbound {
        exceptions: vec![cidr],
    });
    assert_eq!(
        mode,
        VmNetworkMode::OutboundNat {
            plan: OutboundIntent {
                exceptions: vec!["10.0.0.0/8".parse().unwrap()],
                gateway_override: None,
            },
        }
    );
}

fn assert_policy_round_trips(policy: NetworkPolicy) {
    let json = serde_json::to_string(&policy).unwrap();
    let back: NetworkPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(back, policy);
}

/// `#[serde(tag = "kind", rename_all = "snake_case")]` survives round-trip
/// for `NoEgress`. The serde framework itself is tested upstream; this test
/// pins the attribute application.
#[test]
fn network_policy_no_egress_round_trips() {
    assert_policy_round_trips(NetworkPolicy::NoEgress);
}

/// `#[serde(tag = "kind", rename_all = "snake_case")]` survives round-trip
/// for `AllowOutbound`. The serde framework itself is tested upstream; this
/// test pins the attribute application.
#[test]
fn network_policy_allow_outbound_round_trips() {
    assert_policy_round_trips(NetworkPolicy::AllowOutbound {
        exceptions: vec!["192.168.1.0/24".parse().unwrap()],
    });
}
