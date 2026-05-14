use std::net::Ipv4Addr;
use std::path::Path;

use m80_net_mode::OutboundIntent;
use m80_net_outbound::{
    apply_outbound_nat_policy_with_ops, outbound_nat_filter_chain, outbound_nat_rule_comment,
    permanent_deny_cidrs, planned_bridge_state, planned_vm_network_state, NetError,
    PolicyCommandOutput, PolicyOps, SetupPhase, VmNetworkStateRecord,
};

#[test]
fn sysctl_ip_forward_set_before_rules() {
    let state = ready_state();
    let mut ops = RecordingPolicyOps::default();

    apply_outbound_nat_policy_with_ops(&mut ops, &state).unwrap();

    let sysctl = ops.run_index("sysctl -w net.ipv4.ip_forward=1");
    let restore = ops.run_index("iptables-restore -w --noflush");
    assert!(sysctl < restore);
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
fn policy_rules_install_with_one_iptables_restore_batch() {
    let state = ready_state();
    let mut ops = RecordingPolicyOps::default();

    apply_outbound_nat_policy_with_ops(&mut ops, &state).unwrap();

    assert_eq!(ops.restore_inputs.len(), 1);
    assert_eq!(ops.command_outputs_matching("-S").len(), 5);
    assert!(ops.restore_inputs[0].contains("*filter\n"));
    assert!(ops.restore_inputs[0].contains("*nat\n"));
    assert!(!ops
        .runs
        .iter()
        .any(|run| run.starts_with("iptables -w -t filter -A")
            || run.starts_with("iptables -w -t filter -I")
            || run.starts_with("iptables -w -t nat -A")));
}

#[test]
fn iptables_restore_failure_aborts_policy_install() {
    let state = ready_state();
    let mut ops = RecordingPolicyOps {
        fail_restore: true,
        ..RecordingPolicyOps::default()
    };

    let err = apply_outbound_nat_policy_with_ops(&mut ops, &state).unwrap_err();

    assert!(
        matches!(err, NetError::NetworkCommandFailed { program, .. } if program == "iptables-restore")
    );
    assert!(ops.rules.is_empty());
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
fn icmp_rejected_before_default_accept() {
    let state = ready_state();
    let chain = outbound_nat_filter_chain(&state);
    let comment = outbound_nat_rule_comment(&state);
    let mut ops = RecordingPolicyOps::default();

    apply_outbound_nat_policy_with_ops(&mut ops, &state).unwrap();

    let rules = ops.chain_rules("filter", &chain);
    let icmp_index = rules
        .iter()
        .position(|rule| {
            rule == &vec![
                "-p",
                "icmp",
                "-m",
                "comment",
                "--comment",
                &comment,
                "-j",
                "REJECT",
            ]
        })
        .expect("missing ICMP reject rule");
    let default_accept_index = rules
        .iter()
        .position(|rule| rule == &vec!["-m", "comment", "--comment", &comment, "-j", "ACCEPT"])
        .expect("missing default accept");

    assert!(icmp_index < default_accept_index);
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

    let rules = ops.chain_rules("filter", "FORWARD");
    assert!(rules.contains(&vec![
        "-i",
        &state.bridge.bridge_name,
        "-s",
        &guest,
        "-m",
        "comment",
        "--comment",
        &comment,
        "-j",
        &chain
    ]));
    assert!(rules.contains(&vec![
        "-o",
        &state.bridge.bridge_name,
        "-d",
        &guest,
        "-m",
        "comment",
        "--comment",
        &comment,
        "-j",
        "REJECT"
    ]));
    assert!(rules.contains(&vec![
        "-o",
        &state.bridge.bridge_name,
        "-d",
        &guest,
        "-m",
        "conntrack",
        "--ctstate",
        "RELATED,ESTABLISHED",
        "-m",
        "comment",
        "--comment",
        &comment,
        "-j",
        "ACCEPT"
    ]));
    assert!(
        !rules.iter().any(
            |rule| rule.windows(2).any(|pair| pair == ["-i", &state.tap_name])
                || rule.windows(2).any(|pair| pair == ["-o", &state.tap_name])
        ),
        "routed bridge traffic must not be keyed by the TAP device"
    );
}

#[test]
fn nat_postrouting_masquerade_for_guest_source() {
    let state = ready_state();
    let comment = outbound_nat_rule_comment(&state);
    let mut ops = RecordingPolicyOps::default();

    apply_outbound_nat_policy_with_ops(&mut ops, &state).unwrap();

    assert!(ops.chain_rules("nat", "POSTROUTING").contains(&vec![
        "-s",
        &format!("{}/32", state.guest_ipv4),
        "-m",
        "comment",
        "--comment",
        &comment,
        "-j",
        "MASQUERADE"
    ]));
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
    };
    let bridge = planned_bridge_state(run_root)
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
    command_outputs: Vec<String>,
    runs: Vec<String>,
    restore_inputs: Vec<String>,
    chains: Vec<(String, String)>,
    rules: Vec<InstalledRule>,
    fail_sysctl: bool,
    fail_restore: bool,
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

    fn command_outputs_matching(&self, operation: &str) -> Vec<&String> {
        self.command_outputs
            .iter()
            .filter(|command| command.contains(&format!(" {operation} ")))
            .collect()
    }

    fn apply_restore_input(&mut self, input: &str) {
        let mut table = None::<String>;
        for line in input.lines().map(str::trim).filter(|line| !line.is_empty()) {
            if let Some(next_table) = line.strip_prefix('*') {
                table = Some(next_table.to_owned());
                continue;
            }
            if line == "COMMIT" {
                table = None;
                continue;
            }
            let Some(current_table) = table.as_ref() else {
                panic!("restore rule outside table: {line}");
            };
            let parts = line
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            match parts.first().map(String::as_str) {
                Some("-A") => self.rules.push(InstalledRule {
                    table: current_table.clone(),
                    chain: parts[1].clone(),
                    spec: parts[2..].to_vec(),
                }),
                Some("-I") => self.rules.push(InstalledRule {
                    table: current_table.clone(),
                    chain: parts[1].clone(),
                    spec: parts[3..].to_vec(),
                }),
                other => panic!("unexpected restore operation {other:?} in {line}"),
            }
        }
    }
}

impl PolicyOps for RecordingPolicyOps {
    fn command_output(
        &mut self,
        program: &str,
        args: &[String],
    ) -> Result<PolicyCommandOutput, NetError> {
        self.command_outputs
            .push(format!("{program} {}", args.join(" ")));
        if program != "iptables" {
            return Ok(PolicyCommandOutput::success(""));
        }
        let table = &args[2];
        let op = &args[3];
        let chain = &args[4];
        match op.as_str() {
            "-S" => {
                if !self.has_chain(table, chain) && !is_builtin_chain(table, chain) {
                    return Ok(PolicyCommandOutput::failure(
                        "No chain/target/match by that name",
                    ));
                }
                let mut stdout = if is_builtin_chain(table, chain) {
                    format!("-P {chain} ACCEPT\n")
                } else {
                    format!("-N {chain}\n")
                };
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

    fn run_command_input(
        &mut self,
        program: &str,
        args: &[String],
        stdin: &str,
    ) -> Result<(), NetError> {
        self.runs.push(format!("{program} {}", args.join(" ")));
        assert_eq!(program, "iptables-restore");
        assert_eq!(args, ["-w", "--noflush"]);
        self.restore_inputs.push(stdin.to_owned());
        if self.fail_restore {
            return Err(NetError::NetworkCommandFailed {
                program: "iptables-restore".to_owned(),
                stderr: "restore failed".to_owned(),
            });
        }
        self.apply_restore_input(stdin);
        Ok(())
    }
}

fn is_builtin_chain(table: &str, chain: &str) -> bool {
    matches!(
        (table, chain),
        ("filter", "FORWARD") | ("nat", "POSTROUTING")
    )
}

#[derive(Debug)]
struct InstalledRule {
    table: String,
    chain: String,
    spec: Vec<String>,
}
