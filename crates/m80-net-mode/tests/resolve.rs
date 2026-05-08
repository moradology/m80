//! Integration tests for [`m80_net_mode::resolve`].
//!
//! Beads: m80-xbn.1.* (resolver semantics), m80-xbn.2.* (single-seam decision).

use ipnet::Ipv4Net;
use m80_net_mode::{resolve, NetworkPolicy, OutboundIntent, VmNetworkMode};
use std::net::Ipv4Addr;
use std::path::PathBuf;

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

#[test]
fn join_netns_carries_path_through_resolver() {
    let policy = join_netns_policy("/var/run/netns/m80-test");
    let mode = resolve(&policy);
    assert_eq!(
        mode,
        VmNetworkMode::JoinNetns {
            netns_path: PathBuf::from("/var/run/netns/m80-test"),
            tap_name: "tapm80test".to_owned(),
            guest_mac: "02:00:00:00:80:01".to_owned(),
            guest_ipv4: "10.80.0.2/24".parse().unwrap(),
            gateway_ipv4: Ipv4Addr::new(10, 80, 0, 1),
            dns_resolvers: vec![Ipv4Addr::new(10, 80, 0, 1)],
        }
    );
}

#[test]
fn compromised_vmm_network_boundary_is_explicit_join_netns_only() {
    assert_eq!(resolve(&NetworkPolicy::NoEgress), VmNetworkMode::NoEgress);

    assert!(matches!(
        resolve(&NetworkPolicy::AllowOutbound { exceptions: vec![] }),
        VmNetworkMode::OutboundNat { .. }
    ));

    assert_eq!(
        resolve(&join_netns_policy("/var/run/netns/m80-security")),
        VmNetworkMode::JoinNetns {
            netns_path: PathBuf::from("/var/run/netns/m80-security"),
            tap_name: "tapm80test".to_owned(),
            guest_mac: "02:00:00:00:80:01".to_owned(),
            guest_ipv4: "10.80.0.2/24".parse().unwrap(),
            gateway_ipv4: Ipv4Addr::new(10, 80, 0, 1),
            dns_resolvers: vec![Ipv4Addr::new(10, 80, 0, 1)],
        }
    );
}

fn join_netns_policy(netns_path: &str) -> NetworkPolicy {
    NetworkPolicy::JoinNetns {
        netns_path: PathBuf::from(netns_path),
        tap_name: "tapm80test".to_owned(),
        guest_mac: "02:00:00:00:80:01".to_owned(),
        guest_ipv4: "10.80.0.2/24".parse().unwrap(),
        gateway_ipv4: Ipv4Addr::new(10, 80, 0, 1),
        dns_resolvers: vec![Ipv4Addr::new(10, 80, 0, 1)],
    }
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

/// `#[serde(tag = "kind", rename_all = "snake_case")]` survives round-trip
/// for `JoinNetns`.
#[test]
fn network_policy_join_netns_round_trips() {
    assert_policy_round_trips(join_netns_policy("/var/run/netns/m80-test"));
}
