use std::net::Ipv4Addr;
use std::path::Path;

use m80_net_mode::OutboundIntent;
use m80_net_outbound::{
    cleanup_vm_with_ops, derive_tap_name, planned_bridge_state, planned_vm_network_state,
    realize_bridge_and_tap_with_ops_for_routes, write_bridge_state, write_vm_network_state_record,
    LinkOps, NetError, PolicyCommandOutput, PolicyOps, SetupPhase, VmNetworkStateRecord,
};

const DEFAULT_ONLY_ROUTES: &str = "\
Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT\n\
eth0\t00000000\t0102000A\t0003\t0\t0\t100\t00000000\t0\t0\t0\n";

#[test]
fn bridge_removed_only_when_no_peer_references() {
    let temp = tempfile::tempdir().unwrap();
    let mut first = ready_state(temp.path(), "vm-a");
    let second = ready_state(temp.path(), "vm-b");
    first.bridge = second.bridge.clone();
    write_bridge_state(temp.path(), &second.bridge).unwrap();
    write_vm_network_state_record(&first.run_dir, &first).unwrap();
    write_vm_network_state_record(&second.run_dir, &second).unwrap();
    let mut links = RecordingLinkOps::with_existing(&[
        &first.tap_name,
        &second.tap_name,
        &second.bridge.bridge_name,
    ]);
    let mut policy = NoopPolicyOps;

    cleanup_vm_with_ops(&mut links, &mut policy, "vm-a", temp.path()).unwrap();

    assert!(temp.path().join("outbound-bridge-state.json").exists());
    assert!(!links.deleted(&second.bridge.bridge_name));

    cleanup_vm_with_ops(&mut links, &mut policy, "vm-b", temp.path()).unwrap();

    assert!(!temp.path().join("outbound-bridge-state.json").exists());
    assert!(links.deleted(&second.bridge.bridge_name));
}

#[test]
fn startup_scavenges_orphan_bridge_when_unused() {
    let temp = tempfile::tempdir().unwrap();
    let bridge = planned_bridge_state(temp.path())
        .unwrap()
        .with_phase(SetupPhase::Ready);
    write_bridge_state(temp.path(), &bridge).unwrap();
    let mut links = RecordingLinkOps::with_existing(&[&bridge.bridge_name]);
    let mut policy = NoopPolicyOps;

    cleanup_vm_with_ops(&mut links, &mut policy, "missing-vm", temp.path()).unwrap();

    assert!(!temp.path().join("outbound-bridge-state.json").exists());
    assert!(links.deleted(&bridge.bridge_name));
}

#[test]
fn orphan_tap_detected_and_cleaned() {
    let temp = tempfile::tempdir().unwrap();
    let bridge = planned_bridge_state(temp.path())
        .unwrap()
        .with_phase(SetupPhase::Ready);
    let tap_name = derive_tap_name(temp.path(), "missing-vm");
    write_bridge_state(temp.path(), &bridge).unwrap();
    let mut links = RecordingLinkOps::with_existing(&[&bridge.bridge_name, &tap_name]);
    let mut policy = NoopPolicyOps;

    cleanup_vm_with_ops(&mut links, &mut policy, "missing-vm", temp.path()).unwrap();

    assert!(links.deleted(&tap_name));
    assert!(links.deleted(&bridge.bridge_name));
    assert!(!temp.path().join("outbound-bridge-state.json").exists());
}

#[test]
fn malformed_peer_state_surfaces_error() {
    let temp = tempfile::tempdir().unwrap();
    let state = ready_state(temp.path(), "vm-a");
    write_bridge_state(temp.path(), &state.bridge).unwrap();
    write_vm_network_state_record(&state.run_dir, &state).unwrap();
    let peer = temp.path().join("vm-b");
    std::fs::create_dir(&peer).unwrap();
    std::fs::write(peer.join("network-state.json"), b"{not-json").unwrap();
    let mut links = RecordingLinkOps::with_existing(&[&state.tap_name, &state.bridge.bridge_name]);
    let mut policy = NoopPolicyOps;

    let err = cleanup_vm_with_ops(&mut links, &mut policy, "vm-a", temp.path()).unwrap_err();

    assert!(matches!(err, NetError::InvalidNetworkState { .. }));
    assert!(!links.deleted(&state.bridge.bridge_name));
}

#[test]
fn bridge_owner_mismatch_blocks_bridge_delete() {
    let temp = tempfile::tempdir().unwrap();
    let state = ready_state(temp.path(), "vm-a");
    let mut foreign_bridge = state.bridge.clone();
    foreign_bridge.bridge_name = "brfcforeign01".to_owned();
    write_bridge_state(temp.path(), &foreign_bridge).unwrap();
    write_vm_network_state_record(&state.run_dir, &state).unwrap();
    let mut links = RecordingLinkOps::with_existing(&[&state.tap_name, &state.bridge.bridge_name]);
    let mut policy = NoopPolicyOps;

    let err = cleanup_vm_with_ops(&mut links, &mut policy, "vm-a", temp.path()).unwrap_err();

    assert!(matches!(err, NetError::NetworkAllocationConflict { .. }));
    assert!(state.run_dir.join("network-state.json").exists());
    assert!(!links.deleted(&state.bridge.bridge_name));
}

#[test]
fn crash_mid_vm_does_not_break_new_startup() {
    let temp = tempfile::tempdir().unwrap();
    let intent = intent();
    let bridge = planned_bridge_state(temp.path())
        .unwrap()
        .with_phase(SetupPhase::Ready);
    write_bridge_state(temp.path(), &bridge).unwrap();
    let mut links = RecordingLinkOps::with_existing(&[&bridge.bridge_name]);
    let mut policy = NoopPolicyOps;

    cleanup_vm_with_ops(&mut links, &mut policy, "missing-vm", temp.path()).unwrap();
    let run_dir = temp.path().join("vm-b");
    std::fs::create_dir(&run_dir).unwrap();
    realize_bridge_and_tap_with_ops_for_routes(
        &mut links,
        &intent,
        "vm-b",
        temp.path(),
        &run_dir,
        DEFAULT_ONLY_ROUTES,
    )
    .unwrap();

    assert!(links.deleted(&bridge.bridge_name));
    assert!(links
        .operations
        .iter()
        .any(|op| op == &format!("create_bridge {}", bridge.bridge_name)));
}

fn ready_state(run_root: &Path, vm_id: &str) -> VmNetworkStateRecord {
    let run_dir = run_root.join(vm_id);
    std::fs::create_dir(&run_dir).unwrap();
    let intent = intent();
    let bridge = planned_bridge_state(run_root)
        .unwrap()
        .with_phase(SetupPhase::Ready);
    let mut state = planned_vm_network_state(&intent, vm_id, run_root, &run_dir, bridge);
    state.setup_phase = SetupPhase::Ready;
    state.dns_resolvers = vec![Ipv4Addr::new(1, 1, 1, 1)];
    state.runtime_rootfs_configured = true;
    state
}

fn intent() -> OutboundIntent {
    OutboundIntent {
        exceptions: Vec::new(),
    }
}

struct NoopPolicyOps;

impl PolicyOps for NoopPolicyOps {
    fn command_output(
        &mut self,
        program: &str,
        _args: &[String],
    ) -> Result<PolicyCommandOutput, NetError> {
        assert_eq!(program, "iptables");
        Ok(PolicyCommandOutput::failure("absent"))
    }

    fn run_command(&mut self, _program: &str, _args: &[String]) -> Result<(), NetError> {
        Ok(())
    }

    fn run_command_input(
        &mut self,
        _program: &str,
        _args: &[String],
        _stdin: &str,
    ) -> Result<(), NetError> {
        Ok(())
    }
}

#[derive(Default)]
struct RecordingLinkOps {
    operations: Vec<String>,
    existing: Vec<String>,
}

impl RecordingLinkOps {
    fn with_existing(names: &[&str]) -> Self {
        Self {
            operations: Vec::new(),
            existing: names.iter().map(|name| (*name).to_owned()).collect(),
        }
    }

    fn deleted(&self, name: &str) -> bool {
        self.operations
            .iter()
            .any(|op| op == &format!("delete_link_if_exists {name}"))
    }
}

impl LinkOps for RecordingLinkOps {
    fn create_bridge(&mut self, name: &str) -> Result<(), NetError> {
        self.operations.push(format!("create_bridge {name}"));
        self.existing.push(name.to_owned());
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
        self.existing.push(name.to_owned());
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
        if self.existing.iter().any(|existing| existing == name) {
            self.operations
                .push(format!("delete_link_if_exists {name}"));
            self.existing.retain(|existing| existing != name);
        }
        Ok(())
    }

    fn link_exists(&mut self, name: &str) -> Result<bool, NetError> {
        Ok(self.existing.iter().any(|existing| existing == name))
    }

    fn link_has_ipv4_address(
        &mut self,
        _link_name: &str,
        _address: Ipv4Addr,
        _prefix_len: u8,
    ) -> Result<bool, NetError> {
        Ok(true)
    }
}
