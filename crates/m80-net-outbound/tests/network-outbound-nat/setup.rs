use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::{Arc, Barrier};

use m80_net_outbound::{
    bridge_state_path, derive_guest_addressing, planned_bridge_state, planned_vm_network_state,
    read_bridge_state, read_vm_network_state_record, realize_bridge_and_tap_with_ops_for_routes,
    vm_network_state_path, write_bridge_state, write_vm_network_state_record, LinkOps, NetError,
    OutboundIntent, RealizedNetwork, SetupPhase, BRIDGE_STATE_FILE, NETWORK_STATE_FILE,
};

const DEFAULT_ONLY_ROUTES: &str = "\
Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT\n\
eth0\t00000000\t0102000A\t0003\t0\t0\t100\t00000000\t0\t0\t0\n";

#[test]
fn tap_creation_without_ip_binary_then_rtnetlink_attach() {
    let source = include_str!("../../src/link_ops.rs");

    assert_contains(source, "fn create_tap");
    assert_contains(source, "tun::create");
    assert_contains(source, "set_link_mac");
    assert_contains(source, "attach_link_to_bridge");
    assert_contains(source, "set_link_up");
    assert_contains(source, "rtnetlink");
}

#[test]
fn bridge_tap_setup_has_no_ip_shellout_path() {
    let source = include_str!("../../src/link_ops.rs");

    for forbidden in forbidden_ip_shellout_strings() {
        assert!(
            !source.contains(&forbidden),
            "link_ops.rs must not contain forbidden shellout text {forbidden:?}"
        );
    }
}

fn forbidden_ip_shellout_strings() -> Vec<String> {
    vec![
        ["Command::new", "(\"ip\")"].join(""),
        ["Command::new", "(\"", "/sbin", "/ip", "\")"].join(""),
        ["/sbin", "/ip"].join(""),
        ["ip", " tuntap"].join(""),
        ["ip", " link"].join(""),
        ["ip", " addr"].join(""),
    ]
}

fn assert_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected link_ops.rs to contain {needle:?}"
    );
}

#[test]
fn bridge_state_file_is_atomic_and_at_run_root() {
    let temp = tempfile::tempdir().unwrap();
    let intent = intent_with_exception();
    let planned = planned_bridge_state(temp.path(), &intent).unwrap();

    write_bridge_state(temp.path(), &planned).unwrap();

    let path = bridge_state_path(temp.path());
    assert_eq!(path, temp.path().join(BRIDGE_STATE_FILE));
    let json = read_json(&path);
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["setup_phase"], "planned");
    assert_eq!(json["run_root"], temp.path().to_string_lossy().as_ref());
    assert_eq!(json["run_root_digest"], planned.run_root_digest);
    assert_eq!(json["bridge_name"], planned.bridge_name);
    assert_eq!(json["cidr"], planned.cidr.to_string());
    assert_eq!(json["gateway_ipv4"], planned.gateway_ipv4.to_string());
    assert_only_state_file(temp.path(), BRIDGE_STATE_FILE);
    assert_eq!(read_bridge_state(temp.path()).unwrap(), planned);

    let ready = planned.with_phase(SetupPhase::Ready);
    write_bridge_state(temp.path(), &ready).unwrap();
    assert_eq!(read_json(&path)["setup_phase"], "ready");
    assert_eq!(read_bridge_state(temp.path()).unwrap(), ready);
}

#[test]
fn vm_network_state_is_atomic_with_phase_transition() {
    let temp = tempfile::tempdir().unwrap();
    let run_dir = temp.path().join("vm-123");
    std::fs::create_dir(&run_dir).unwrap();
    let intent = intent_with_exception();
    let bridge = planned_bridge_state(temp.path(), &intent)
        .unwrap()
        .with_phase(SetupPhase::Ready);
    let planned =
        planned_vm_network_state(&intent, "vm-123", temp.path(), &run_dir, bridge.clone());

    write_vm_network_state_record(&run_dir, &planned).unwrap();

    let path = vm_network_state_path(&run_dir);
    assert_eq!(path, run_dir.join(NETWORK_STATE_FILE));
    let json = read_json(&path);
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["setup_phase"], "planned");
    assert_eq!(json["vm_id"], "vm-123");
    assert_eq!(json["run_dir"], run_dir.to_string_lossy().as_ref());
    assert_eq!(json["bridge"]["setup_phase"], "ready");
    assert_eq!(json["tap_name"], planned.tap_name);
    assert_eq!(json["guest_mac"], planned.guest_mac);
    assert_eq!(json["guest_ipv4"], planned.guest_ipv4.to_string());
    assert_eq!(json["private_ipv4_exceptions"][0], "10.42.0.0/16");
    assert_eq!(json["dns_resolvers"].as_array().unwrap().len(), 0);
    assert_eq!(json["runtime_rootfs_configured"], false);
    assert_only_state_file(&run_dir, NETWORK_STATE_FILE);

    let ready = planned.with_phase(SetupPhase::Ready);
    write_vm_network_state_record(&run_dir, &ready).unwrap();
    assert_eq!(read_json(&path)["setup_phase"], "ready");
    assert_eq!(read_vm_network_state_record(&run_dir).unwrap(), ready);
}

#[test]
fn bridge_setup_is_idempotent_with_matching_state() {
    let temp = tempfile::tempdir().unwrap();
    let run_dir = temp.path().join("vm-123");
    std::fs::create_dir(&run_dir).unwrap();
    let intent = intent_with_exception();
    let ready_bridge = planned_bridge_state(temp.path(), &intent)
        .unwrap()
        .with_phase(SetupPhase::Ready);
    write_bridge_state(temp.path(), &ready_bridge).unwrap();
    let mut ops = RecordingLinkOps::with_link_address(true);

    let realized = realize_for_test(&mut ops, &intent, "vm-123", temp.path(), &run_dir).unwrap();

    assert_eq!(realized.bridge_name, ready_bridge.bridge_name);
    assert_eq!(realized.bridge_cidr, ready_bridge.cidr);
    assert_eq!(read_bridge_state(temp.path()).unwrap(), ready_bridge);
    assert_eq!(
        ops.operations,
        [
            format!(
                "link_has_ipv4_address {} {}/{}",
                ready_bridge.bridge_name,
                ready_bridge.gateway_ipv4,
                ready_bridge.cidr.prefix_len()
            ),
            format!("create_tap {}", realized.tap_name),
            format!("set_link_mac {} {}", realized.tap_name, realized.guest_mac),
            format!(
                "attach_link_to_bridge {} {}",
                realized.tap_name, ready_bridge.bridge_name
            ),
            format!("set_link_up {}", realized.tap_name),
        ]
    );
    let vm_state = read_vm_network_state_record(&run_dir).unwrap();
    assert_eq!(vm_state.setup_phase, SetupPhase::Ready);
    assert_eq!(vm_state.tap_name, realized.tap_name);
}

#[test]
fn planned_bridge_state_recovers_existing_kernel_bridge_without_recreate() {
    let temp = tempfile::tempdir().unwrap();
    let run_dir = temp.path().join("vm-123");
    std::fs::create_dir(&run_dir).unwrap();
    let intent = intent_with_exception();
    let planned_bridge = planned_bridge_state(temp.path(), &intent).unwrap();
    write_bridge_state(temp.path(), &planned_bridge).unwrap();
    let mut ops = RecordingLinkOps::with_existing_link_address(true);

    let realized = realize_for_test(&mut ops, &intent, "vm-123", temp.path(), &run_dir).unwrap();

    assert_eq!(
        read_bridge_state(temp.path()).unwrap(),
        planned_bridge.clone().with_phase(SetupPhase::Ready)
    );
    assert!(!ops
        .operations
        .iter()
        .any(|op| op == &format!("create_bridge {}", planned_bridge.bridge_name)));
    assert_eq!(
        ops.operations[..3],
        [
            format!("link_exists {}", planned_bridge.bridge_name),
            format!(
                "link_has_ipv4_address {} {}/{}",
                planned_bridge.bridge_name,
                planned_bridge.gateway_ipv4,
                planned_bridge.cidr.prefix_len()
            ),
            format!("set_link_up {}", planned_bridge.bridge_name),
        ]
    );
    assert_eq!(realized.bridge_name, planned_bridge.bridge_name);
}

#[test]
fn planned_bridge_state_recreates_kernel_dropped_bridge() {
    let temp = tempfile::tempdir().unwrap();
    let run_dir = temp.path().join("vm-123");
    std::fs::create_dir(&run_dir).unwrap();
    let intent = intent_with_exception();
    let planned_bridge = planned_bridge_state(temp.path(), &intent).unwrap();
    write_bridge_state(temp.path(), &planned_bridge).unwrap();
    let mut ops = RecordingLinkOps::default();

    let realized = realize_for_test(&mut ops, &intent, "vm-123", temp.path(), &run_dir).unwrap();

    assert_eq!(
        read_bridge_state(temp.path()).unwrap(),
        planned_bridge.clone().with_phase(SetupPhase::Ready)
    );
    assert_eq!(
        ops.operations[..5],
        [
            format!("link_exists {}", planned_bridge.bridge_name),
            format!("create_bridge {}", planned_bridge.bridge_name),
            format!(
                "link_has_ipv4_address {} {}/{}",
                planned_bridge.bridge_name,
                planned_bridge.gateway_ipv4,
                planned_bridge.cidr.prefix_len()
            ),
            format!(
                "add_ipv4_address {} {}/{}",
                planned_bridge.bridge_name,
                planned_bridge.gateway_ipv4,
                planned_bridge.cidr.prefix_len()
            ),
            format!("set_link_up {}", planned_bridge.bridge_name),
        ]
    );
    assert_eq!(realized.bridge_name, planned_bridge.bridge_name);
}

#[test]
fn host_route_collision_returns_typed_error_pre_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let run_dir = temp.path().join("vm-123");
    std::fs::create_dir(&run_dir).unwrap();
    let intent = intent_with_exception();
    let planned_bridge = planned_bridge_state(temp.path(), &intent).unwrap();
    let host_routes = host_route_for(planned_bridge.cidr);
    let mut ops = RecordingLinkOps::default();

    let err = realize_bridge_and_tap_with_ops_for_routes(
        &mut ops,
        &intent,
        "vm-123",
        temp.path(),
        &run_dir,
        &host_routes,
    )
    .unwrap_err();

    assert!(matches!(err, NetError::HostRouteCollision { .. }));
    assert!(ops.operations.is_empty());
    assert!(!bridge_state_path(temp.path()).exists());
    assert!(!vm_network_state_path(&run_dir).exists());
}

#[test]
fn concurrent_launch_no_ipv4_collision() {
    let temp = tempfile::tempdir().unwrap();
    let run_root = temp.path().to_path_buf();
    let (first_vm, second_vm, colliding_ip) = colliding_vm_ids(&run_root);
    let barrier = Arc::new(Barrier::new(2));

    let first = {
        let run_root = run_root.clone();
        let barrier = Arc::clone(&barrier);
        let vm_id = first_vm.clone();
        std::thread::spawn(move || {
            let run_dir = run_root.join(&vm_id);
            std::fs::create_dir(&run_dir).unwrap();
            let mut ops = RecordingLinkOps::with_link_address(true);
            let intent = intent_with_exception();
            barrier.wait();
            realize_for_test(&mut ops, &intent, &vm_id, &run_root, &run_dir)
        })
    };
    let second = {
        let run_root = run_root.clone();
        let barrier = Arc::clone(&barrier);
        let vm_id = second_vm.clone();
        std::thread::spawn(move || {
            let run_dir = run_root.join(&vm_id);
            std::fs::create_dir(&run_dir).unwrap();
            let mut ops = RecordingLinkOps::with_link_address(true);
            let intent = intent_with_exception();
            barrier.wait();
            realize_for_test(&mut ops, &intent, &vm_id, &run_root, &run_dir)
        })
    };

    let outcomes = [first.join().unwrap(), second.join().unwrap()];

    assert!(
        outcomes
            .iter()
            .any(|outcome| matches!(outcome, Err(NetError::GuestIpv4Collision { .. }))),
        "at least one colliding setup must fail closed"
    );
    let ready_states = [first_vm, second_vm]
        .into_iter()
        .filter_map(|vm_id| read_vm_network_state_record(&run_root.join(vm_id)).ok())
        .collect::<Vec<_>>();
    assert!(
        ready_states
            .iter()
            .filter(|state| state.guest_ipv4 == colliding_ip)
            .count()
            <= 1,
        "colliding guest IP {colliding_ip} must not be assigned to two ready VM states"
    );
}

#[test]
fn failed_launch_after_bridge_cleans_bridge() {
    let temp = tempfile::tempdir().unwrap();
    let run_dir = temp.path().join("vm-123");
    std::fs::create_dir(&run_dir).unwrap();
    let intent = intent_with_exception();
    let planned_bridge = planned_bridge_state(temp.path(), &intent).unwrap();
    let mut ops = RecordingLinkOps {
        fail_create_tap: true,
        ..RecordingLinkOps::default()
    };

    let err = realize_for_test(&mut ops, &intent, "vm-123", temp.path(), &run_dir).unwrap_err();

    assert!(matches!(
        err,
        NetError::TapOperationFailed {
            operation: "create tap",
            ..
        }
    ));
    assert!(ops.operations.contains(&format!(
        "delete_link_if_exists {}",
        planned_bridge.bridge_name
    )));
    assert!(!bridge_state_path(temp.path()).exists());
    assert!(!vm_network_state_path(&run_dir).exists());
}

#[test]
fn bridge_state_mismatch_fails_before_link_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let run_dir = temp.path().join("vm-123");
    std::fs::create_dir(&run_dir).unwrap();
    let intent = intent_with_exception();
    let mut foreign = planned_bridge_state(temp.path(), &intent).unwrap();
    foreign.bridge_name = "brfcforeign01".to_owned();
    write_bridge_state(temp.path(), &foreign.with_phase(SetupPhase::Ready)).unwrap();
    let mut ops = RecordingLinkOps::with_link_address(true);

    let err = realize_for_test(&mut ops, &intent, "vm-123", temp.path(), &run_dir).unwrap_err();

    assert!(matches!(err, NetError::BridgeOwnershipMismatch));
    assert!(ops.operations.is_empty());
    assert!(!vm_network_state_path(&run_dir).exists());
}

fn intent_with_exception() -> OutboundIntent {
    OutboundIntent {
        exceptions: vec!["10.42.0.0/16".parse().unwrap()],
        gateway_override: None,
    }
}

fn realize_for_test(
    ops: &mut impl LinkOps,
    intent: &OutboundIntent,
    vm_id: &str,
    run_root: &std::path::Path,
    run_dir: &std::path::Path,
) -> Result<RealizedNetwork, NetError> {
    realize_bridge_and_tap_with_ops_for_routes(
        ops,
        intent,
        vm_id,
        run_root,
        run_dir,
        DEFAULT_ONLY_ROUTES,
    )
}

fn colliding_vm_ids(run_root: &std::path::Path) -> (String, String, Ipv4Addr) {
    let mut seen = HashMap::new();
    for index in 0..10_000 {
        let vm_id = format!("vm-collision-{index}");
        let (guest_ip, _) = derive_guest_addressing(run_root, &vm_id);
        if let Some(first_vm) = seen.insert(guest_ip, vm_id.clone()) {
            return (first_vm, vm_id, guest_ip);
        }
    }
    panic!("expected a guest IP collision within 10k deterministic VM ids");
}

fn host_route_for(cidr: ipnet::Ipv4Net) -> String {
    format!(
        "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT\n\
eth0\t{}\t00000000\t0001\t0\t0\t0\t{}\t0\t0\t0\n",
        proc_route_hex(cidr.network()),
        proc_route_hex(cidr.netmask())
    )
}

fn proc_route_hex(ip: Ipv4Addr) -> String {
    let [a, b, c, d] = ip.octets();
    format!("{d:02X}{c:02X}{b:02X}{a:02X}")
}

fn read_json(path: &std::path::Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn assert_only_state_file(dir: &std::path::Path, file_name: &str) {
    let files = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(files, [file_name]);
}

#[derive(Default)]
struct RecordingLinkOps {
    operations: Vec<String>,
    link_exists: bool,
    link_has_ipv4_address: bool,
    fail_create_tap: bool,
}

impl RecordingLinkOps {
    fn with_link_address(link_has_ipv4_address: bool) -> Self {
        Self {
            operations: Vec::new(),
            link_exists: false,
            link_has_ipv4_address,
            fail_create_tap: false,
        }
    }

    fn with_existing_link_address(link_has_ipv4_address: bool) -> Self {
        Self {
            operations: Vec::new(),
            link_exists: true,
            link_has_ipv4_address,
            fail_create_tap: false,
        }
    }
}

impl LinkOps for RecordingLinkOps {
    fn create_bridge(&mut self, name: &str) -> Result<(), NetError> {
        self.operations.push(format!("create_bridge {name}"));
        Ok(())
    }

    fn add_ipv4_address(
        &mut self,
        link_name: &str,
        address: Ipv4Addr,
        prefix_len: u8,
    ) -> Result<(), NetError> {
        self.operations.push(format!(
            "add_ipv4_address {link_name} {address}/{prefix_len}"
        ));
        Ok(())
    }

    fn create_tap(&mut self, name: &str) -> Result<(), NetError> {
        self.operations.push(format!("create_tap {name}"));
        if self.fail_create_tap {
            return Err(NetError::TapOperationFailed {
                operation: "create tap",
                source: std::io::Error::other("injected tap failure"),
            });
        }
        Ok(())
    }

    fn set_link_mac(&mut self, name: &str, mac: [u8; 6]) -> Result<(), NetError> {
        self.operations.push(format!(
            "set_link_mac {name} {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
        ));
        Ok(())
    }

    fn attach_link_to_bridge(
        &mut self,
        link_name: &str,
        bridge_name: &str,
    ) -> Result<(), NetError> {
        self.operations
            .push(format!("attach_link_to_bridge {link_name} {bridge_name}"));
        Ok(())
    }

    fn set_link_up(&mut self, name: &str) -> Result<(), NetError> {
        self.operations.push(format!("set_link_up {name}"));
        Ok(())
    }

    fn delete_link_if_exists(&mut self, name: &str) -> Result<(), NetError> {
        self.operations
            .push(format!("delete_link_if_exists {name}"));
        Ok(())
    }

    fn link_exists(&mut self, name: &str) -> Result<bool, NetError> {
        self.operations.push(format!("link_exists {name}"));
        Ok(self.link_exists)
    }

    fn link_has_ipv4_address(
        &mut self,
        link_name: &str,
        address: Ipv4Addr,
        prefix_len: u8,
    ) -> Result<bool, NetError> {
        self.operations.push(format!(
            "link_has_ipv4_address {link_name} {address}/{prefix_len}"
        ));
        Ok(self.link_has_ipv4_address)
    }
}
