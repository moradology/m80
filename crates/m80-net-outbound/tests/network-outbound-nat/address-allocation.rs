use std::net::Ipv4Addr;
use std::path::Path;

use ipnet::Ipv4Net;
use m80_net_outbound::{
    derive_bridge_cidr, derive_bridge_name, derive_guest_addressing, derive_tap_name,
};

#[test]
fn bridge_name_is_brfc_plus_11_hex_of_run_root_digest() {
    let run_root = Path::new("/var/run/m80");

    let bridge_name = derive_bridge_name(run_root);

    assert_eq!(bridge_name, "brfc2d568e06361");
    assert_eq!(bridge_name.len(), 15);
    assert_eq!(derive_bridge_name(run_root), bridge_name);
}

#[test]
fn bridge_cidr_is_172_o2_o3_slash_24_with_octet_formula() {
    let run_root = Path::new("/var/run/m80");

    let cidr = derive_bridge_cidr(run_root);

    assert_eq!(cidr, "172.29.86.0/24".parse::<Ipv4Net>().unwrap());
    assert_eq!(cidr.network(), Ipv4Addr::new(172, 29, 86, 0));
    assert_eq!(cidr.broadcast(), Ipv4Addr::new(172, 29, 86, 255));
}

#[test]
fn bridge_cidrs_stay_within_172_16_slash_12() {
    for root in [
        "/var/run/m80",
        "/tmp/m80/run-root-a",
        "/tmp/m80/run-root-b",
        "/srv/m80/customer/workload",
    ] {
        let cidr = derive_bridge_cidr(Path::new(root));
        let [o1, o2, _o3, o4] = cidr.network().octets();

        assert_eq!(o1, 172);
        assert!((16..=31).contains(&o2), "{root} produced {cidr}");
        assert_eq!(o4, 0);
        assert_eq!(cidr.prefix_len(), 24);
    }
}

#[test]
fn tap_name_is_tfc_plus_12_hex_of_vm_digest() {
    let run_root = Path::new("/var/run/m80");

    let tap_name = derive_tap_name(run_root, "vm-123");

    assert_eq!(tap_name, "tfc8dc14a89e07e");
    assert_eq!(tap_name.len(), 15);
    assert_eq!(derive_tap_name(run_root, "vm-123"), tap_name);
    assert_ne!(derive_tap_name(run_root, "vm-456"), tap_name);
}

#[test]
fn guest_ipv4_is_deterministic_per_run_root_and_vm_id() {
    let run_root = Path::new("/tmp/m80/run-root-a");

    let (guest_ipv4, _) = derive_guest_addressing(run_root, "sandbox-alpha");

    assert_eq!(guest_ipv4, Ipv4Addr::new(172, 23, 2, 30));
    assert_eq!(
        derive_guest_addressing(run_root, "sandbox-alpha").0,
        guest_ipv4
    );
    assert_ne!(
        derive_guest_addressing(run_root, "sandbox-beta").0,
        guest_ipv4
    );
    let host_octet = guest_ipv4.octets()[3];
    assert!((2..=254).contains(&host_octet));
}

#[test]
fn guest_mac_is_locally_administered_unicast() {
    let run_root = Path::new("/tmp/m80/run-root-b");

    let (_, guest_mac) = derive_guest_addressing(run_root, "sandbox-beta");

    assert_eq!(guest_mac, "02:8d:9d:42:db:fd");
    let first_octet = u8::from_str_radix(guest_mac.split(':').next().unwrap(), 16).unwrap();
    assert_eq!(first_octet & 0b0000_0001, 0, "MAC must be unicast");
    assert_ne!(
        first_octet & 0b0000_0010,
        0,
        "MAC must be locally administered"
    );
    assert_eq!(guest_mac.split(':').count(), 6);
}
