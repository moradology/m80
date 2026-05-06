use std::net::Ipv4Addr;

use ipnet::Ipv4Net;
use m80_net_outbound::{
    reject_guest_ipv4_collision, reject_host_route_collision_from_proc_net_route, NetError,
    NETWORK_STATE_FILE,
};

#[test]
fn reject_guest_ipv4_collision_with_sibling() {
    let temp = tempfile::tempdir().unwrap();
    let peer_dir = temp.path().join("peer-vm");
    std::fs::create_dir(&peer_dir).unwrap();
    std::fs::write(
        peer_dir.join(NETWORK_STATE_FILE),
        r#"{
            "vm_id": "peer-vm",
            "bridge": { "cidr": "172.23.2.0/24" },
            "guest_ipv4": "172.23.2.30"
        }"#,
    )
    .unwrap();

    let err = reject_guest_ipv4_collision(
        temp.path(),
        "current-vm",
        "172.23.2.0/24".parse().unwrap(),
        Ipv4Addr::new(172, 23, 2, 30),
    )
    .unwrap_err();

    assert!(matches!(
        err,
        NetError::GuestIpv4Collision { peer_vm_id } if peer_vm_id == "peer-vm"
    ));
}

#[test]
fn reject_guest_ipv4_collision_ignores_same_vm_and_other_cidr() {
    let temp = tempfile::tempdir().unwrap();
    for (dir, vm_id, cidr, ip) in [
        ("same-vm", "current-vm", "172.23.2.0/24", "172.23.2.30"),
        ("other-cidr", "peer-vm", "172.23.3.0/24", "172.23.2.30"),
    ] {
        let vm_dir = temp.path().join(dir);
        std::fs::create_dir(&vm_dir).unwrap();
        std::fs::write(
            vm_dir.join(NETWORK_STATE_FILE),
            format!(
                r#"{{
                    "vm_id": "{vm_id}",
                    "bridge": {{ "cidr": "{cidr}" }},
                    "guest_ipv4": "{ip}"
                }}"#
            ),
        )
        .unwrap();
    }

    reject_guest_ipv4_collision(
        temp.path(),
        "current-vm",
        "172.23.2.0/24".parse().unwrap(),
        Ipv4Addr::new(172, 23, 2, 30),
    )
    .unwrap();
}

#[test]
fn reject_host_route_collision_against_proc_net_route() {
    let routes = route_fixture("eth0", "000217AC", "00FFFFFF");

    let err = reject_host_route_collision_from_proc_net_route(
        "172.23.2.0/24".parse().unwrap(),
        None,
        &routes,
    )
    .unwrap_err();

    assert!(matches!(
        err,
        NetError::HostRouteCollision { existing } if existing == "eth0 172.23.2.0/24"
    ));
}

#[test]
fn allowed_same_interface_route_is_not_a_collision() {
    let routes = route_fixture("brfcfixture", "000217AC", "00FFFFFF");

    reject_host_route_collision_from_proc_net_route(
        "172.23.2.0/24".parse().unwrap(),
        Some("brfcfixture"),
        &routes,
    )
    .unwrap();
}

#[test]
fn default_route_and_non_overlapping_routes_do_not_collide() {
    let routes = format!(
        "{}\n{}\n{}",
        route_header(),
        route_line("eth0", "00000000", "00000000"),
        route_line("eth1", "000317AC", "00FFFFFF")
    );

    reject_host_route_collision_from_proc_net_route(
        "172.23.2.0/24".parse::<Ipv4Net>().unwrap(),
        None,
        &routes,
    )
    .unwrap();
}

#[test]
fn collision_is_hard_error_no_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let peer_dir = temp.path().join("peer-vm");
    std::fs::create_dir(&peer_dir).unwrap();
    std::fs::write(
        peer_dir.join(NETWORK_STATE_FILE),
        r#"{
            "vm_id": "peer-vm",
            "bridge": { "cidr": "172.23.2.0/24" },
            "guest_ipv4": "172.23.2.30"
        }"#,
    )
    .unwrap();

    let planned_cidr = "172.23.2.0/24".parse().unwrap();
    let planned_ip = Ipv4Addr::new(172, 23, 2, 30);
    let first = reject_guest_ipv4_collision(temp.path(), "current-vm", planned_cidr, planned_ip);
    let second = reject_guest_ipv4_collision(temp.path(), "current-vm", planned_cidr, planned_ip);

    assert!(matches!(
        first,
        Err(NetError::GuestIpv4Collision { peer_vm_id }) if peer_vm_id == "peer-vm"
    ));
    assert!(matches!(
        second,
        Err(NetError::GuestIpv4Collision { peer_vm_id }) if peer_vm_id == "peer-vm"
    ));
}

fn route_fixture(interface: &str, destination: &str, mask: &str) -> String {
    format!(
        "{}\n{}\n",
        route_header(),
        route_line(interface, destination, mask)
    )
}

fn route_header() -> &'static str {
    "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT"
}

fn route_line(interface: &str, destination: &str, mask: &str) -> String {
    format!("{interface}\t{destination}\t00000000\t0001\t0\t0\t0\t{mask}\t0\t0\t0")
}
