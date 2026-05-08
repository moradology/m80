use std::net::Ipv4Addr;
use std::path::Path;

use m80_net_outbound::{
    apply_outbound_nat_policy_with_ops, outbound_nat_filter_chain, outbound_nat_rule_comment,
    permanent_deny_cidrs, planned_bridge_state, planned_vm_network_state, NetError, OutboundIntent,
    PolicyCommandOutput, PolicyOps, SetupPhase, VmNetworkStateRecord,
};

#[test]
fn sysctl_ip_forward_set_before_rules() {
    let state = ready_state();
    let mut ops = RecordingPolicyOps::default();

    apply_outbound_nat_policy_with_ops(&mut ops, &state).unwrap();

    let sysctl = ops.run_index("sysctl -w net.ipv4.ip_forward=1");
    let forward = ops.run_index("iptables -w -t filter -I FORWARD 1");
    let nat = ops.run_index("iptables -w -t nat -A POSTROUTING");
    assert!(sysctl < forward);
    assert!(sysctl < nat);
}

#[test]
fn sysctl_failure_aborts_before_rule_install() {
    let state = ready_state();
    let mut ops = RecordingPolicyOps {
        fail_sysctl: true,
        ..RecordingPolicyOps::default()
    };

    let err = apply_outbound_nat_policy_with_ops(&mut ops, &state).unwrap_err();

    assert!(matches!(err, NetError::NetworkCommandFailed { program, .. } if program == "sysctl"));
    assert_eq!(
        ops.runs_after("sysctl -w net.ipv4.ip_forward=1")
            .filter(|command| command.starts_with("iptables -w -t filter -A"))
            .count(),
        0
    );
    assert_eq!(
        ops.runs_after("sysctl -w net.ipv4.ip_forward=1")
            .filter(|command| command.starts_with("iptables -w -t filter -I FORWARD"))
            .count(),
        0
    );
    assert_eq!(
        ops.runs_after("sysctl -w net.ipv4.ip_forward=1")
            .filter(|command| command.starts_with("iptables -w -t nat -A POSTROUTING"))
            .count(),
        0
    );
}

#[test]
fn per_vm_filter_chain_named_tfw_plus_12_hex() {
    let state = ready_state();
    let chain = outbound_nat_filter_chain(&state);
    let mut ops = RecordingPolicyOps::default();

    apply_outbound_nat_policy_with_ops(&mut ops, &state).unwrap();

    assert_eq!(chain.len(), 15);
    assert!(chain.starts_with("tfw"));
    assert!(ops
        .runs
        .contains(&format!("iptables -w -t filter -N {chain}")));
}

#[test]
fn dns_accept_per_admitted_resolver_udp_and_tcp() {
    let state = ready_state();
    let chain = outbound_nat_filter_chain(&state);
    let comment = outbound_nat_rule_comment(&state);
    let mut ops = RecordingPolicyOps::default();

    apply_outbound_nat_policy_with_ops(&mut ops, &state).unwrap();

    let rules = ops.chain_rules("filter", &chain);
    assert_eq!(
        &rules[..4],
        [
            vec![
                "-p",
                "udp",
                "-d",
                "1.1.1.1",
                "--dport",
                "53",
                "-m",
                "comment",
                "--comment",
                &comment,
                "-j",
                "ACCEPT"
            ],
            vec![
                "-p",
                "tcp",
                "-d",
                "1.1.1.1",
                "--dport",
                "53",
                "-m",
                "comment",
                "--comment",
                &comment,
                "-j",
                "ACCEPT"
            ],
            vec![
                "-p",
                "udp",
                "-d",
                "8.8.8.8",
                "--dport",
                "53",
                "-m",
                "comment",
                "--comment",
                &comment,
                "-j",
                "ACCEPT"
            ],
            vec![
                "-p",
                "tcp",
                "-d",
                "8.8.8.8",
                "--dport",
                "53",
                "-m",
                "comment",
                "--comment",
                &comment,
                "-j",
                "ACCEPT"
            ],
        ]
    );
}

#[test]
fn reject_other_port_53() {
    let state = ready_state();
    let chain = outbound_nat_filter_chain(&state);
    let comment = outbound_nat_rule_comment(&state);
    let mut ops = RecordingPolicyOps::default();

    apply_outbound_nat_policy_with_ops(&mut ops, &state).unwrap();

    let rules = ops.chain_rules("filter", &chain);
    assert_eq!(
        rules[4],
        vec![
            "-p",
            "udp",
            "--dport",
            "53",
            "-m",
            "comment",
            "--comment",
            &comment,
            "-j",
            "REJECT"
        ]
    );
    assert_eq!(
        rules[5],
        vec![
            "-p",
            "tcp",
            "--dport",
            "53",
            "-m",
            "comment",
            "--comment",
            &comment,
            "-j",
            "REJECT"
        ]
    );
}

#[test]
fn bounded_private_exceptions_accepted() {
    let state = ready_state();
    let chain = outbound_nat_filter_chain(&state);
    let comment = outbound_nat_rule_comment(&state);
    let mut ops = RecordingPolicyOps::default();

    apply_outbound_nat_policy_with_ops(&mut ops, &state).unwrap();

    let rules = ops.chain_rules("filter", &chain);
    assert!(rules.contains(&vec![
        "-d",
        "10.42.0.0/24",
        "-m",
        "comment",
        "--comment",
        &comment,
        "-j",
        "ACCEPT"
    ]));
    assert!(rules.contains(&vec![
        "-d",
        "192.168.50.0/24",
        "-m",
        "comment",
        "--comment",
        &comment,
        "-j",
        "ACCEPT"
    ]));
}

#[test]
fn permanent_deny_list_includes_bridge_cidr() {
    let state = ready_state();
    let chain = outbound_nat_filter_chain(&state);
    let comment = outbound_nat_rule_comment(&state);
    let mut ops = RecordingPolicyOps::default();

    apply_outbound_nat_policy_with_ops(&mut ops, &state).unwrap();

    let rules = ops.chain_rules("filter", &chain);
    for cidr in permanent_deny_cidrs(state.bridge.cidr) {
        assert!(
            rules.contains(&vec![
                "-d",
                &cidr.to_string(),
                "-m",
                "comment",
                "--comment",
                &comment,
                "-j",
                "REJECT"
            ]),
            "missing permanent deny rule for {cidr}"
        );
    }
}

#[test]
fn default_accept_after_deny_list() {
    let state = ready_state();
    let chain = outbound_nat_filter_chain(&state);
    let comment = outbound_nat_rule_comment(&state);
    let mut ops = RecordingPolicyOps::default();

    apply_outbound_nat_policy_with_ops(&mut ops, &state).unwrap();

    assert_eq!(
        ops.chain_rules("filter", &chain).last().unwrap(),
        &vec!["-m", "comment", "--comment", &comment, "-j", "ACCEPT"]
    );
}

#[test]
fn forward_inserts_route_guest_through_filter_chain() {
    let state = ready_state();
    let chain = outbound_nat_filter_chain(&state);
    let comment = outbound_nat_rule_comment(&state);
    let guest = format!("{}/32", state.guest_ipv4);
    let mut ops = RecordingPolicyOps::default();

    apply_outbound_nat_policy_with_ops(&mut ops, &state).unwrap();

    assert!(ops.runs.contains(&format!(
        "iptables -w -t filter -I FORWARD 1 -i {} -s {} -m comment --comment {} -j {}",
        state.bridge.bridge_name, guest, comment, chain
    )));
    assert!(ops.runs.contains(&format!(
        "iptables -w -t filter -I FORWARD 1 -o {} -d {} -m comment --comment {} -j REJECT",
        state.bridge.bridge_name, guest, comment
    )));
    assert!(ops.runs.contains(&format!(
        "iptables -w -t filter -I FORWARD 1 -o {} -d {} -m conntrack --ctstate RELATED,ESTABLISHED -m comment --comment {} -j ACCEPT",
        state.bridge.bridge_name, guest, comment
    )));
    assert!(
        !ops.runs
            .iter()
            .any(|run| run.starts_with("iptables -w -t filter -I FORWARD 1")
                && (run.contains(&format!("-i {}", state.tap_name))
                    || run.contains(&format!("-o {}", state.tap_name)))),
        "routed bridge traffic must not be keyed by the TAP device"
    );
}

#[test]
fn nat_postrouting_masquerade_for_guest_source() {
    let state = ready_state();
    let comment = outbound_nat_rule_comment(&state);
    let mut ops = RecordingPolicyOps::default();

    apply_outbound_nat_policy_with_ops(&mut ops, &state).unwrap();

    assert!(ops.runs.contains(&format!(
        "iptables -w -t nat -A POSTROUTING -s {}/32 -m comment --comment {} -j MASQUERADE",
        state.guest_ipv4, comment
    )));
}

#[test]
fn policy_reapply_skips_existing_chain_and_rules() {
    let state = ready_state();
    let mut ops = RecordingPolicyOps::default();

    apply_outbound_nat_policy_with_ops(&mut ops, &state).unwrap();
    let first_run_count = ops.runs.len();
    apply_outbound_nat_policy_with_ops(&mut ops, &state).unwrap();

    assert_eq!(ops.runs.len(), first_run_count + 1);
    assert_eq!(ops.runs.last().unwrap(), "sysctl -w net.ipv4.ip_forward=1");
}

fn ready_state() -> VmNetworkStateRecord {
    let run_root = Path::new("/tmp/m80-outbound-run");
    let run_dir = run_root.join("vm-a");
    let intent = OutboundIntent {
        exceptions: vec![
            "10.42.0.0/24".parse().unwrap(),
            "192.168.50.0/24".parse().unwrap(),
        ],
        gateway_override: None,
    };
    let bridge = planned_bridge_state(run_root, &intent)
        .unwrap()
        .with_phase(SetupPhase::Ready);
    let mut state = planned_vm_network_state(&intent, "vm-a", run_root, &run_dir, bridge);
    state.setup_phase = SetupPhase::Ready;
    state.dns_resolvers = vec![Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(8, 8, 8, 8)];
    state.runtime_rootfs_configured = true;
    state
}

#[derive(Default)]
struct RecordingPolicyOps {
    runs: Vec<String>,
    chains: Vec<(String, String)>,
    rules: Vec<InstalledRule>,
    fail_sysctl: bool,
}

impl RecordingPolicyOps {
    fn run_index(&self, prefix: &str) -> usize {
        self.runs
            .iter()
            .position(|command| command.starts_with(prefix))
            .unwrap_or_else(|| panic!("missing run command with prefix {prefix:?}"))
    }

    fn runs_after(&self, prefix: &str) -> impl Iterator<Item = &String> {
        let index = self.run_index(prefix);
        self.runs[index + 1..].iter()
    }

    fn chain_rules(&self, table: &str, chain: &str) -> Vec<Vec<&str>> {
        self.rules
            .iter()
            .filter(|rule| rule.table == table && rule.chain == chain)
            .map(|rule| rule.spec.iter().map(String::as_str).collect::<Vec<_>>())
            .collect()
    }

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
        if program != "iptables" {
            return Ok(PolicyCommandOutput::success(""));
        }
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
        if program == "sysctl" && self.fail_sysctl {
            return Err(NetError::NetworkCommandFailed {
                program: "sysctl".to_owned(),
                stderr: "permission denied".to_owned(),
            });
        }
        if program != "iptables" {
            return Ok(());
        }
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
