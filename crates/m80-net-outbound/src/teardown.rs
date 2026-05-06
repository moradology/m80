use std::fs;
use std::path::{Path, PathBuf};

use crate::{
    bridge_state_path, link_ops, outbound_nat_filter_chain, outbound_nat_rule_comment,
    read_bridge_state, read_vm_network_state_record, vm_network_state_path, LinkOps, NetError,
    PolicyOps, VmNetworkStateRecord,
};

/// Tear down the network state owned by `vm_id` with real host backends.
pub fn cleanup_vm(vm_id: &str, run_root: &Path) -> Result<(), NetError> {
    let mut link_ops = link_ops::NetlinkLinkOps::new()?;
    let mut policy_ops = crate::iptables::CommandPolicyOps;
    cleanup_vm_with_ops(&mut link_ops, &mut policy_ops, vm_id, run_root)
}

/// Tear down the network state owned by `vm_id` through supplied backends.
pub fn cleanup_vm_with_ops(
    links: &mut impl LinkOps,
    policy_ops: &mut impl PolicyOps,
    vm_id: &str,
    run_root: &Path,
) -> Result<(), NetError> {
    let run_dir = run_root.join(vm_id);
    let state_path = vm_network_state_path(&run_dir);
    if !state_path.exists() {
        return cleanup_orphan_bridge_with_ops(links, run_root);
    }

    let state = read_vm_network_state_record(&run_dir)?;
    validate_cleanup_network_state(&run_dir, run_root, &state)?;
    cleanup_outbound_nat_policy_with_ops(policy_ops, &state)?;
    link_ops::teardown_tap(links, &state.tap_name)?;
    if !other_bridge_users_exist(&state)? {
        validate_bridge_owner_record_for_cleanup(&state)?;
        link_ops::teardown_tap(links, &state.bridge.bridge_name)?;
        remove_file_if_present(&bridge_state_path(&state.bridge.run_root))?;
    }
    remove_file_if_present(&state_path)
}

/// Remove the run-root bridge if no VM state in the run-root references it.
pub fn cleanup_orphan_bridge(run_root: &Path) -> Result<(), NetError> {
    let mut links = link_ops::NetlinkLinkOps::new()?;
    cleanup_orphan_bridge_with_ops(&mut links, run_root)
}

/// Remove the run-root bridge through a supplied link backend when unused.
pub fn cleanup_orphan_bridge_with_ops(
    links: &mut impl LinkOps,
    run_root: &Path,
) -> Result<(), NetError> {
    let bridge_state = bridge_state_path(run_root);
    if !bridge_state.exists() || any_vm_network_states_exist(run_root)? {
        return Ok(());
    }
    let bridge = read_bridge_state(run_root)?;
    if bridge.run_root != run_root {
        return Err(NetError::InvalidNetworkState {
            path: bridge_state,
            detail: format!(
                "bridge state run_root {} does not match cleanup run root {}",
                bridge.run_root.display(),
                run_root.display()
            ),
        });
    }
    link_ops::teardown_tap(links, &bridge.bridge_name)?;
    remove_file_if_present(&bridge_state_path(run_root))
}

/// Remove the iptables policy owned by one VM state.
pub fn cleanup_outbound_nat_policy_with_ops(
    ops: &mut impl PolicyOps,
    state: &VmNetworkStateRecord,
) -> Result<(), NetError> {
    let chain = outbound_nat_filter_chain(state);
    let comment = outbound_nat_rule_comment(state);
    delete_forwarding_entry_rules(ops, state, &chain, &comment)?;
    delete_nat_masquerade_rule(ops, state, &comment)?;
    delete_owned_iptables_chain_rules(ops, "filter", &chain, &comment)?;
    delete_iptables_chain_if_empty(ops, "filter", &chain, &comment)
}

fn delete_forwarding_entry_rules(
    ops: &mut impl PolicyOps,
    state: &VmNetworkStateRecord,
    chain: &str,
    comment: &str,
) -> Result<(), NetError> {
    let guest = format!("{}/32", state.guest_ipv4);
    delete_iptables_rule_if_present(
        ops,
        "filter",
        "FORWARD",
        &[
            "-i",
            &state.tap_name,
            "-s",
            &guest,
            "-m",
            "comment",
            "--comment",
            comment,
            "-j",
            chain,
        ],
    )?;
    delete_iptables_rule_if_present(
        ops,
        "filter",
        "FORWARD",
        &[
            "-o",
            &state.tap_name,
            "-d",
            &guest,
            "-m",
            "comment",
            "--comment",
            comment,
            "-j",
            "REJECT",
        ],
    )?;
    delete_iptables_rule_if_present(
        ops,
        "filter",
        "FORWARD",
        &[
            "-o",
            &state.tap_name,
            "-d",
            &guest,
            "-m",
            "conntrack",
            "--ctstate",
            "RELATED,ESTABLISHED",
            "-m",
            "comment",
            "--comment",
            comment,
            "-j",
            "ACCEPT",
        ],
    )
}

fn delete_nat_masquerade_rule(
    ops: &mut impl PolicyOps,
    state: &VmNetworkStateRecord,
    comment: &str,
) -> Result<(), NetError> {
    let guest = format!("{}/32", state.guest_ipv4);
    delete_iptables_rule_if_present(
        ops,
        "nat",
        "POSTROUTING",
        &[
            "-s",
            &guest,
            "-m",
            "comment",
            "--comment",
            comment,
            "-j",
            "MASQUERADE",
        ],
    )
}

fn delete_owned_iptables_chain_rules(
    ops: &mut impl PolicyOps,
    table: &str,
    chain: &str,
    comment: &str,
) -> Result<(), NetError> {
    let Some(rules) = list_iptables_chain_rules(ops, table, chain)? else {
        return Ok(());
    };
    for rule in rules {
        if !rule.contains(comment) {
            return Err(NetError::ForeignChainRule { rule });
        }
        let split = split_iptables_rule_spec(&rule);
        let refs = split.iter().map(String::as_str).collect::<Vec<_>>();
        delete_iptables_rule_if_present(ops, table, chain, &refs)?;
    }
    Ok(())
}

fn delete_iptables_chain_if_empty(
    ops: &mut impl PolicyOps,
    table: &str,
    chain: &str,
    comment: &str,
) -> Result<(), NetError> {
    let Some(rules) = list_iptables_chain_rules(ops, table, chain)? else {
        return Ok(());
    };
    if let Some(rule) = rules.first() {
        let detail = if rule.contains(comment) {
            "owned policy chain still contains residue after cleanup"
        } else {
            "owned policy chain still contains an unowned rule"
        };
        return Err(NetError::NetworkAllocationConflict {
            path: iptables_state_path(table, chain),
            detail: detail.to_owned(),
        });
    }
    ops.run_command("iptables", &iptables_args(table, "-X", chain, &[]))
}

fn list_iptables_chain_rules(
    ops: &mut impl PolicyOps,
    table: &str,
    chain: &str,
) -> Result<Option<Vec<String>>, NetError> {
    let output = ops.command_output("iptables", &iptables_args(table, "-S", chain, &[]))?;
    if !output.status_success {
        return Ok(None);
    }
    let mut rules = Vec::new();
    for line in output.stdout.lines().map(str::trim) {
        if line.is_empty() || line == format!("-N {chain}") {
            continue;
        }
        let prefix = format!("-A {chain} ");
        let Some(rule) = line.strip_prefix(&prefix) else {
            return Err(NetError::NetworkAllocationConflict {
                path: iptables_state_path(table, chain),
                detail: format!("unexpected iptables-save rule shape {line:?}"),
            });
        };
        rules.push(rule.to_owned());
    }
    Ok(Some(rules))
}

fn delete_iptables_rule_if_present(
    ops: &mut impl PolicyOps,
    table: &str,
    chain: &str,
    rule: &[&str],
) -> Result<(), NetError> {
    let rule_args = rule.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
    for _ in 0..32 {
        let check_args = iptables_args(table, "-C", chain, &rule_args);
        if !ops.command_output("iptables", &check_args)?.status_success {
            return Ok(());
        }
        ops.run_command("iptables", &iptables_args(table, "-D", chain, &rule_args))?;
    }
    Err(NetError::NetworkAllocationConflict {
        path: iptables_state_path(table, chain),
        detail: "owned iptables rule remained after repeated deletion attempts".to_owned(),
    })
}

fn split_iptables_rule_spec(rule: &str) -> Vec<String> {
    rule.split_whitespace()
        .map(|arg| arg.trim_matches('"').to_owned())
        .collect()
}

fn iptables_args(table: &str, operation: &str, chain: &str, rest: &[String]) -> Vec<String> {
    let mut args = vec![
        "-w".to_owned(),
        "-t".to_owned(),
        table.to_owned(),
        operation.to_owned(),
        chain.to_owned(),
    ];
    args.extend(rest.iter().cloned());
    args
}

fn remove_file_if_present(path: &Path) -> Result<(), NetError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(source.into()),
    }
}

fn validate_cleanup_network_state(
    run_dir: &Path,
    run_root: &Path,
    state: &VmNetworkStateRecord,
) -> Result<(), NetError> {
    if state.run_dir != run_dir {
        return Err(NetError::InvalidNetworkState {
            path: vm_network_state_path(run_dir),
            detail: format!(
                "network state run_dir {} does not match VM run directory {}",
                state.run_dir.display(),
                run_dir.display()
            ),
        });
    }
    if state.bridge.run_root != run_root {
        return Err(NetError::InvalidNetworkState {
            path: vm_network_state_path(run_dir),
            detail: format!(
                "network state bridge run_root {} does not match VM run root {}",
                state.bridge.run_root.display(),
                run_root.display()
            ),
        });
    }
    Ok(())
}

fn other_bridge_users_exist(state: &VmNetworkStateRecord) -> Result<bool, NetError> {
    let run_root = &state.bridge.run_root;
    if !run_root.exists() {
        return Ok(false);
    }
    for entry in fs::read_dir(run_root)? {
        let candidate_dir = entry?.path();
        if candidate_dir == state.run_dir || !candidate_dir.is_dir() {
            continue;
        }
        let candidate_state_path = vm_network_state_path(&candidate_dir);
        if !candidate_state_path.exists() {
            continue;
        }
        let Ok(candidate) = read_vm_network_state_record(&candidate_dir) else {
            return Ok(true);
        };
        if candidate.bridge.run_root == state.bridge.run_root
            && candidate.bridge.bridge_name == state.bridge.bridge_name
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn any_vm_network_states_exist(run_root: &Path) -> Result<bool, NetError> {
    if !run_root.exists() {
        return Ok(false);
    }
    for entry in fs::read_dir(run_root)? {
        let candidate_dir = entry?.path();
        if candidate_dir.is_dir() && vm_network_state_path(&candidate_dir).exists() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn validate_bridge_owner_record_for_cleanup(state: &VmNetworkStateRecord) -> Result<(), NetError> {
    let path = bridge_state_path(&state.bridge.run_root);
    if !path.exists() {
        return Ok(());
    }
    let existing = read_bridge_state(&state.bridge.run_root)?;
    if crate::state::bridge_state_matches_identity(&existing, &state.bridge) {
        return Ok(());
    }
    Err(NetError::NetworkAllocationConflict {
        path,
        detail: "bridge owner record does not match cleanup VM bridge identity".to_owned(),
    })
}

fn iptables_state_path(table: &str, chain: &str) -> PathBuf {
    PathBuf::from(format!("iptables:{table}:{chain}"))
}
