use std::net::Ipv4Addr;

use m80_net_outbound::{
    bridge_state_path, planned_bridge_state, planned_vm_network_state, read_bridge_state,
    read_vm_network_state_record, realize_bridge_and_tap_with_ops, vm_network_state_path,
    write_bridge_state, write_vm_network_state_record, LinkOps, NetError, OutboundIntent,
    SetupPhase, BRIDGE_STATE_FILE, NETWORK_STATE_FILE,
};

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

    let realized =
        realize_bridge_and_tap_with_ops(&mut ops, &intent, "vm-123", temp.path(), &run_dir)
            .unwrap();

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

    let realized =
        realize_bridge_and_tap_with_ops(&mut ops, &intent, "vm-123", temp.path(), &run_dir)
            .unwrap();

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
fn bridge_state_mismatch_fails_before_link_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let run_dir = temp.path().join("vm-123");
    std::fs::create_dir(&run_dir).unwrap();
    let intent = intent_with_exception();
    let mut foreign = planned_bridge_state(temp.path(), &intent).unwrap();
    foreign.bridge_name = "brfcforeign01".to_owned();
    write_bridge_state(temp.path(), &foreign.with_phase(SetupPhase::Ready)).unwrap();
    let mut ops = RecordingLinkOps::with_link_address(true);

    let err = realize_bridge_and_tap_with_ops(&mut ops, &intent, "vm-123", temp.path(), &run_dir)
        .unwrap_err();

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
}

impl RecordingLinkOps {
    fn with_link_address(link_has_ipv4_address: bool) -> Self {
        Self {
            operations: Vec::new(),
            link_exists: false,
            link_has_ipv4_address,
        }
    }

    fn with_existing_link_address(link_has_ipv4_address: bool) -> Self {
        Self {
            operations: Vec::new(),
            link_exists: true,
            link_has_ipv4_address,
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
