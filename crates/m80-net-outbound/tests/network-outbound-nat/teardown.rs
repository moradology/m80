use std::net::Ipv4Addr;
use std::path::Path;

use m80_net_outbound::{
    apply_outbound_nat_policy_with_ops, cleanup_outbound_nat_policy_with_ops, cleanup_vm_with_ops,
    outbound_nat_filter_chain, outbound_nat_rule_comment, planned_bridge_state,
    planned_vm_network_state, write_vm_network_state_record, LinkOps, NetError, OutboundIntent,
    PolicyCommandOutput, PolicyOps, SetupPhase, VmNetworkStateRecord,
};

#[test]
fn every_owned_rule_carries_per_vm_comment() {
    let state = ready_state(Path::new("/tmp/m80-teardown-run"), "vm-a");
    let comment = outbound_nat_rule_comment(&state);
    let mut ops = RecordingPolicyOps::default();

    apply_outbound_nat_policy_with_ops(&mut ops, &state).unwrap();

    for rule in ops.rules.iter().filter(|rule| {
        rule.chain == outbound_nat_filter_chain(&state)
            || rule.chain == "FORWARD"
            || rule.chain == "POSTROUTING"
    }) {
        assert!(
            rule.spec
                .windows(2)
                .any(|pair| pair[0] == "--comment" && pair[1] == comment),
            "owned rule must carry comment {comment}: {rule:?}"
        );
    }
}

#[test]
fn cleanup_deletes_only_rules_with_owned_comment() {
    let state = ready_state(Path::new("/tmp/m80-teardown-run"), "vm-a");
    let comment = outbound_nat_rule_comment(&state);
    let mut ops = RecordingPolicyOps::default();
    apply_outbound_nat_policy_with_ops(&mut ops, &state).unwrap();
    ops.rules.push(InstalledRule {
        table: "filter".to_owned(),
        chain: "FORWARD".to_owned(),
        spec: vec!["-j".to_owned(), "ACCEPT".to_owned()],
    });

    cleanup_outbound_nat_policy_with_ops(&mut ops, &state).unwrap();

    assert!(
        ops.rules
            .iter()
            .all(|rule| !rule.spec.iter().any(|arg| arg == &comment)),
        "owned comment must be gone from all policy tables"
    );
    assert!(ops
        .rules
        .iter()
        .any(|rule| rule.spec == ["-j".to_owned(), "ACCEPT".to_owned()]));
}

#[test]
fn cleanup_foreign_rule_in_owned_chain_aborts() {
    let state = ready_state(Path::new("/tmp/m80-teardown-run"), "vm-a");
    let chain = outbound_nat_filter_chain(&state);
    let mut ops = RecordingPolicyOps::default();
    ops.chains.push(("filter".to_owned(), chain.clone()));
    ops.rules.push(InstalledRule {
        table: "filter".to_owned(),
        chain,
        spec: vec!["-j".to_owned(), "ACCEPT".to_owned()],
    });

    let err = cleanup_outbound_nat_policy_with_ops(&mut ops, &state).unwrap_err();

    assert!(matches!(err, NetError::ForeignChainRule { .. }));
}

#[test]
fn chain_deleted_only_when_empty() {
    let state = ready_state(Path::new("/tmp/m80-teardown-run"), "vm-a");
    let chain = outbound_nat_filter_chain(&state);
    let mut ops = RecordingPolicyOps::default();
    apply_outbound_nat_policy_with_ops(&mut ops, &state).unwrap();

    cleanup_outbound_nat_policy_with_ops(&mut ops, &state).unwrap();

    assert!(!ops.has_chain("filter", &chain));
    assert!(ops
        .runs
        .contains(&format!("iptables -w -t filter -X {chain}")));
}

#[test]
fn repeated_cleanup_calls_are_safe() {
    let temp = tempfile::tempdir().unwrap();
    let run_root = temp.path();
    let state = ready_state(run_root, "vm-a");
    write_vm_network_state_record(&state.run_dir, &state).unwrap();
    let mut policy_ops = RecordingPolicyOps::default();
    apply_outbound_nat_policy_with_ops(&mut policy_ops, &state).unwrap();
    let mut link_ops = RecordingLinkOps::with_existing(&state.tap_name);

    cleanup_vm_with_ops(&mut link_ops, &mut policy_ops, "vm-a", run_root).unwrap();
    cleanup_vm_with_ops(&mut link_ops, &mut policy_ops, "vm-a", run_root).unwrap();

    assert_eq!(
        link_ops.operations,
        [format!("delete_link_if_exists {}", state.tap_name)]
    );
    assert!(!state.run_dir.join("network-state.json").exists());
}

#[test]
fn foreign_rule_in_owned_chain_aborts() {
    let state = ready_state(Path::new("/tmp/m80-teardown-run"), "vm-a");
    let chain = outbound_nat_filter_chain(&state);
    let mut ops = RecordingPolicyOps::default();
    ops.chains.push(("filter".to_owned(), chain.clone()));
    ops.rules.push(InstalledRule {
        table: "filter".to_owned(),
        chain,
        spec: vec!["-j".to_owned(), "ACCEPT".to_owned()],
    });

    let err = apply_outbound_nat_policy_with_ops(&mut ops, &state).unwrap_err();

    assert!(matches!(err, NetError::ForeignChainRule { .. }));
    assert!(
        !ops.runs
            .iter()
            .any(|command| command == "sysctl -w net.ipv4.ip_forward=1"),
        "foreign chain rule must abort before sysctl/rule installation"
    );
}

fn ready_state(run_root: &Path, vm_id: &str) -> VmNetworkStateRecord {
    let run_dir = run_root.join(vm_id);
    if run_root.exists() {
        std::fs::create_dir_all(&run_dir).unwrap();
    }
    let intent = OutboundIntent {
        exceptions: vec!["10.42.0.0/24".parse().unwrap()],
        gateway_override: None,
    };
    let bridge = planned_bridge_state(run_root, &intent)
        .unwrap()
        .with_phase(SetupPhase::Ready);
    let mut state = planned_vm_network_state(&intent, vm_id, run_root, &run_dir, bridge);
    state.setup_phase = SetupPhase::Ready;
    state.dns_resolvers = vec![Ipv4Addr::new(1, 1, 1, 1)];
    state.runtime_rootfs_configured = true;
    state
}

#[derive(Default)]
struct RecordingPolicyOps {
    runs: Vec<String>,
    chains: Vec<(String, String)>,
    rules: Vec<InstalledRule>,
}

impl RecordingPolicyOps {
    fn has_chain(&self, table: &str, chain: &str) -> bool {
        self.chains
            .iter()
            .any(|(candidate_table, candidate_chain)| {
                candidate_table == table && candidate_chain == chain
            })
    }

    fn has_rule(&self, table: &str, chain: &str, spec: &[String]) -> bool {
        self.rules
            .iter()
            .any(|rule| rule.table == table && rule.chain == chain && rule.spec.as_slice() == spec)
    }
}

impl PolicyOps for RecordingPolicyOps {
    fn command_output(
        &mut self,
        program: &str,
        args: &[String],
    ) -> Result<PolicyCommandOutput, NetError> {
        assert_eq!(program, "iptables");
        let table = &args[2];
        let op = &args[3];
        let chain = &args[4];
        match op.as_str() {
            "-S" => {
                if !self.has_chain(table, chain) {
                    return Ok(PolicyCommandOutput::failure(
                        "No chain/target/match by that name",
                    ));
                }
                let mut stdout = format!("-N {chain}\n");
                for rule in self
                    .rules
                    .iter()
                    .filter(|rule| rule.table == *table && rule.chain == *chain)
                {
                    stdout.push_str(&format!("-A {chain} {}\n", rule.spec.join(" ")));
                }
                Ok(PolicyCommandOutput::success(stdout))
            }
            "-C" => {
                if self.has_rule(table, chain, &args[5..]) {
                    Ok(PolicyCommandOutput::success(""))
                } else {
                    Ok(PolicyCommandOutput::failure("rule not found"))
                }
            }
            _ => Ok(PolicyCommandOutput::success("")),
        }
    }

    fn run_command(&mut self, program: &str, args: &[String]) -> Result<(), NetError> {
        self.runs.push(format!("{program} {}", args.join(" ")));
        if program == "sysctl" {
            return Ok(());
        }
        assert_eq!(program, "iptables");
        let table = args[2].clone();
        let op = args[3].as_str();
        let chain = args[4].clone();
        match op {
            "-N" => self.chains.push((table, chain)),
            "-A" => self.rules.push(InstalledRule {
                table,
                chain,
                spec: args[5..].to_vec(),
            }),
            "-I" => self.rules.push(InstalledRule {
                table,
                chain,
                spec: args[6..].to_vec(),
            }),
            "-D" => {
                let spec = args[5..].to_vec();
                if let Some(index) = self.rules.iter().position(|rule| {
                    rule.table == table && rule.chain == chain && rule.spec == spec
                }) {
                    self.rules.remove(index);
                }
            }
            "-X" => {
                self.chains.retain(|(candidate_table, candidate_chain)| {
                    candidate_table != &table || candidate_chain != &chain
                });
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Debug)]
struct InstalledRule {
    table: String,
    chain: String,
    spec: Vec<String>,
}

#[derive(Default)]
struct RecordingLinkOps {
    operations: Vec<String>,
    existing: Vec<String>,
}

impl RecordingLinkOps {
    fn with_existing(name: &str) -> Self {
        Self {
            operations: Vec::new(),
            existing: vec![name.to_owned()],
        }
    }
}

impl LinkOps for RecordingLinkOps {
    fn create_bridge(&mut self, _name: &str) -> Result<(), NetError> {
        Ok(())
    }

    fn add_ipv4_address(
        &mut self,
        _link_name: &str,
        _address: Ipv4Addr,
        _prefix_len: u8,
    ) -> Result<(), NetError> {
        Ok(())
    }

    fn create_tap(&mut self, _name: &str) -> Result<(), NetError> {
        Ok(())
    }

    fn set_link_mac(&mut self, _name: &str, _mac: [u8; 6]) -> Result<(), NetError> {
        Ok(())
    }

    fn attach_link_to_bridge(
        &mut self,
        _link_name: &str,
        _bridge_name: &str,
    ) -> Result<(), NetError> {
        Ok(())
    }

    fn set_link_up(&mut self, _name: &str) -> Result<(), NetError> {
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
