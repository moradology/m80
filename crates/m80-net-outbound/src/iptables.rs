//! iptables policy application and cleanup for per-VM outbound NAT.
//! Owns chain creation, DNS-accept/reject sequencing, permanent-deny rules,
//! FORWARD inserts, NAT masquerade, and comment-tagged teardown.

use std::collections::HashMap;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ipnet::Ipv4Net;
use sha2::{Digest, Sha256};

use crate::{NetError, SetupPhase, VmNetworkStateRecord, RULE_COMMENT_PREFIX};

pub(crate) const TCP_SYN_CONN_LIMIT_PER_VM: u16 = 256;

/// Output returned by a host network policy command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PolicyCommandOutput {
    /// Whether the process exited successfully.
    pub(crate) status_success: bool,
    /// Captured stdout as best-effort UTF-8.
    pub(crate) stdout: String,
    /// Captured stderr as best-effort UTF-8.
    pub(crate) stderr: String,
}

#[cfg(test)]
impl PolicyCommandOutput {
    /// Construct a successful command output with stdout.
    pub(crate) fn success(stdout: impl Into<String>) -> Self {
        Self {
            status_success: true,
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    /// Construct a failed command output with stderr.
    pub(crate) fn failure(stderr: impl Into<String>) -> Self {
        Self {
            status_success: false,
            stdout: String::new(),
            stderr: stderr.into(),
        }
    }
}

/// Host command seam for IPv4 forwarding and iptables policy operations.
pub(crate) trait PolicyOps {
    /// Run a command and return stdout/stderr without interpreting the exit status.
    fn command_output(
        &mut self,
        program: &str,
        args: &[String],
    ) -> Result<PolicyCommandOutput, NetError>;

    /// Run a command whose non-zero exit aborts policy installation.
    fn run_command(&mut self, program: &str, args: &[String]) -> Result<(), NetError> {
        let output = self.command_output(program, args)?;
        if output.status_success {
            return Ok(());
        }
        Err(NetError::NetworkCommandFailed {
            program: program.to_owned(),
            stderr: output.stderr,
        })
    }

    /// Run a command with stdin whose non-zero exit aborts policy installation.
    fn run_command_input(
        &mut self,
        program: &str,
        args: &[String],
        stdin: &str,
    ) -> Result<(), NetError>;
}

pub(crate) struct CommandPolicyOps;

impl PolicyOps for CommandPolicyOps {
    fn command_output(
        &mut self,
        program: &str,
        args: &[String],
    ) -> Result<PolicyCommandOutput, NetError> {
        let output = Command::new(program).args(args).output()?;
        Ok(PolicyCommandOutput {
            status_success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    fn run_command_input(
        &mut self,
        program: &str,
        args: &[String],
        stdin: &str,
    ) -> Result<(), NetError> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut child_stdin = child.stdin.take().expect("stdin was piped");
        let write_result = child_stdin.write_all(stdin.as_bytes());
        drop(child_stdin);
        let output = child.wait_with_output()?;
        if output.status.success() {
            write_result?;
            return Ok(());
        }
        Err(NetError::NetworkCommandFailed {
            program: program.to_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

/// Apply the outbound NAT iptables policy with real host commands.
pub fn apply_outbound_nat_policy(state: &VmNetworkStateRecord) -> Result<(), NetError> {
    let mut ops = CommandPolicyOps;
    apply_outbound_nat_policy_with_ops(&mut ops, state)
}

/// Apply the outbound NAT iptables policy through a supplied command backend.
pub(crate) fn apply_outbound_nat_policy_with_ops(
    ops: &mut impl PolicyOps,
    state: &VmNetworkStateRecord,
) -> Result<(), NetError> {
    validate_ready_state(state)?;
    let chain = outbound_nat_filter_chain(state);
    let comment = outbound_nat_rule_comment(state);
    ensure_iptables_chain(ops, "filter", &chain)?;
    reject_foreign_iptables_chain_rules(ops, "filter", &chain, &comment)?;
    ensure_host_network_sysctls(ops, state)?;
    let expected = expected_policy_rules(state, &chain, &comment);
    restore_missing_policy_rules(ops, &expected)
}

/// Return the per-VM filter chain name.
///
/// Format: `tfw` followed by the first 12 hex chars of `sha256(run_dir)`.
#[must_use]
pub(crate) fn outbound_nat_filter_chain(state: &VmNetworkStateRecord) -> String {
    format!("tfw{}", &digest_path(&state.run_dir)[..12])
}

/// Return the per-VM iptables rule comment.
///
/// Format: `<RULE_COMMENT_PREFIX>:<first 12 hex chars sha256(run_root)>:<tap_name>`.
#[must_use]
pub(crate) fn outbound_nat_rule_comment(state: &VmNetworkStateRecord) -> String {
    format!(
        "{}:{}:{}",
        RULE_COMMENT_PREFIX,
        &digest_path(&state.bridge.run_root)[..12],
        state.tap_name
    )
}

/// Return the permanent-deny IPv4 CIDRs for one VM policy.
#[must_use]
pub(crate) fn permanent_deny_cidrs(bridge_cidr: Ipv4Net) -> Vec<Ipv4Net> {
    let mut cidrs = vec![
        "0.0.0.0/8",
        "10.0.0.0/8",
        "100.64.0.0/10",
        "127.0.0.0/8",
        "169.254.0.0/16",
        "172.16.0.0/12",
        "192.168.0.0/16",
        "192.0.2.0/24",
        "198.18.0.0/15",
        "198.51.100.0/24",
        "203.0.113.0/24",
        "224.0.0.0/4",
        "240.0.0.0/4",
    ]
    .into_iter()
    .map(|cidr| cidr.parse().expect("static CIDR is valid"))
    .collect::<Vec<_>>();
    cidrs.push(bridge_cidr);
    cidrs
}

fn validate_ready_state(state: &VmNetworkStateRecord) -> Result<(), NetError> {
    if state.setup_phase != SetupPhase::Ready {
        return Err(invalid_state(
            &state.run_dir,
            "VM network state must be ready before policy installation",
        ));
    }
    if state.bridge.setup_phase != SetupPhase::Ready {
        return Err(invalid_state(
            &state.run_dir,
            "bridge state must be ready before policy installation",
        ));
    }
    if !state.runtime_rootfs_configured || state.dns_resolvers.is_empty() {
        return Err(invalid_state(
            &state.run_dir,
            "OutboundNat policy requires runtime rootfs DNS configuration first",
        ));
    }
    Ok(())
}

fn ensure_host_network_sysctls(
    ops: &mut impl PolicyOps,
    state: &VmNetworkStateRecord,
) -> Result<(), NetError> {
    ensure_ipv4_forwarding(ops)?;
    disable_ipv6_on_link(ops, &state.bridge.bridge_name)?;
    disable_ipv6_on_link(ops, &state.host_veth_name)
}

fn ensure_ipv4_forwarding(ops: &mut impl PolicyOps) -> Result<(), NetError> {
    ops.run_command(
        "sysctl",
        &["-w".to_owned(), "net.ipv4.ip_forward=1".to_owned()],
    )
}

fn disable_ipv6_on_link(ops: &mut impl PolicyOps, link_name: &str) -> Result<(), NetError> {
    ops.run_command(
        "sysctl",
        &[
            "-w".to_owned(),
            format!("net.ipv6.conf.{link_name}.disable_ipv6=1"),
        ],
    )
}

fn ensure_iptables_chain(
    ops: &mut impl PolicyOps,
    table: &str,
    chain: &str,
) -> Result<(), NetError> {
    if ops
        .command_output("iptables", &iptables_args(table, "-S", chain, &[]))?
        .status_success
    {
        return Ok(());
    }
    ops.run_command("iptables", &iptables_args(table, "-N", chain, &[]))
}

fn reject_foreign_iptables_chain_rules(
    ops: &mut impl PolicyOps,
    table: &str,
    chain: &str,
    comment: &str,
) -> Result<(), NetError> {
    let output = ops.command_output("iptables", &iptables_args(table, "-S", chain, &[]))?;
    if !output.status_success {
        return Err(invalid_state(
            iptables_state_path(table, chain),
            "owned policy chain disappeared after creation",
        ));
    }
    for line in output.stdout.lines().map(str::trim) {
        if line.is_empty() || line == format!("-N {chain}") {
            continue;
        }
        if !line.contains(comment) {
            return Err(NetError::ForeignChainRule {
                rule: line.to_owned(),
            });
        }
    }
    Ok(())
}

fn ensure_filter_chain_rules(
    state: &VmNetworkStateRecord,
    chain: &str,
    comment: &str,
) -> Vec<PlannedRule> {
    let mut rules = Vec::new();
    for resolver in &state.dns_resolvers {
        rules.push(PlannedRule::append(
            "filter",
            chain,
            vec![
                "-p".into(),
                "udp".into(),
                "-d".into(),
                resolver.to_string(),
                "--dport".into(),
                "53".into(),
                "-m".into(),
                "comment".into(),
                "--comment".into(),
                comment.into(),
                "-j".into(),
                "ACCEPT".into(),
            ],
        ));
        rules.push(PlannedRule::append(
            "filter",
            chain,
            vec![
                "-p".into(),
                "tcp".into(),
                "-d".into(),
                resolver.to_string(),
                "--dport".into(),
                "53".into(),
                "-m".into(),
                "comment".into(),
                "--comment".into(),
                comment.into(),
                "-j".into(),
                "ACCEPT".into(),
            ],
        ));
    }

    for protocol in ["udp", "tcp"] {
        rules.push(PlannedRule::append(
            "filter",
            chain,
            vec![
                "-p".into(),
                protocol.into(),
                "--dport".into(),
                "53".into(),
                "-m".into(),
                "comment".into(),
                "--comment".into(),
                comment.into(),
                "-j".into(),
                "REJECT".into(),
            ],
        ));
    }

    for exception in &state.private_ipv4_exceptions {
        rules.push(PlannedRule::append(
            "filter",
            chain,
            vec![
                "-d".into(),
                exception.to_string(),
                "-m".into(),
                "comment".into(),
                "--comment".into(),
                comment.into(),
                "-j".into(),
                "ACCEPT".into(),
            ],
        ));
    }

    for cidr in permanent_deny_cidrs(state.bridge.cidr) {
        rules.push(PlannedRule::append(
            "filter",
            chain,
            vec![
                "-d".into(),
                cidr.to_string(),
                "-m".into(),
                "comment".into(),
                "--comment".into(),
                comment.into(),
                "-j".into(),
                "REJECT".into(),
            ],
        ));
    }

    rules.push(PlannedRule::append(
        "filter",
        chain,
        vec![
            "-p".into(),
            "icmp".into(),
            "-m".into(),
            "comment".into(),
            "--comment".into(),
            comment.into(),
            "-j".into(),
            "REJECT".into(),
        ],
    ));

    rules.push(PlannedRule::append(
        "filter",
        chain,
        vec![
            "-m".into(),
            "comment".into(),
            "--comment".into(),
            comment.into(),
            "-j".into(),
            "ACCEPT".into(),
        ],
    ));
    rules
}

fn ensure_forwarding_entry_rules(
    state: &VmNetworkStateRecord,
    chain: &str,
    comment: &str,
) -> Vec<PlannedRule> {
    let guest = format!("{}/32", state.guest_ipv4);
    vec![
        PlannedRule::insert(
            "filter",
            "FORWARD",
            vec![
                "-i".into(),
                state.bridge.bridge_name.clone(),
                "-s".into(),
                guest.clone(),
                "-m".into(),
                "comment".into(),
                "--comment".into(),
                comment.into(),
                "-j".into(),
                chain.into(),
            ],
        ),
        PlannedRule::insert(
            "filter",
            "FORWARD",
            vec![
                "-o".into(),
                state.bridge.bridge_name.clone(),
                "-d".into(),
                guest.clone(),
                "-m".into(),
                "comment".into(),
                "--comment".into(),
                comment.into(),
                "-j".into(),
                "REJECT".into(),
            ],
        ),
        PlannedRule::insert(
            "filter",
            "FORWARD",
            vec![
                "-o".into(),
                state.bridge.bridge_name.clone(),
                "-d".into(),
                guest.clone(),
                "-m".into(),
                "conntrack".into(),
                "--ctstate".into(),
                "RELATED,ESTABLISHED".into(),
                "-m".into(),
                "comment".into(),
                "--comment".into(),
                comment.into(),
                "-j".into(),
                "ACCEPT".into(),
            ],
        ),
        PlannedRule::insert(
            "filter",
            "FORWARD",
            vec![
                "-i".into(),
                state.bridge.bridge_name.clone(),
                "-s".into(),
                guest,
                "-p".into(),
                "tcp".into(),
                "--syn".into(),
                "-m".into(),
                "connlimit".into(),
                "--connlimit-above".into(),
                TCP_SYN_CONN_LIMIT_PER_VM.to_string(),
                "--connlimit-mask".into(),
                "32".into(),
                "-m".into(),
                "comment".into(),
                "--comment".into(),
                comment.into(),
                "-j".into(),
                "REJECT".into(),
            ],
        ),
    ]
}

fn ensure_nat_masquerade_rule(state: &VmNetworkStateRecord, comment: &str) -> PlannedRule {
    PlannedRule::append(
        "nat",
        "POSTROUTING",
        vec![
            "-s".into(),
            format!("{}/32", state.guest_ipv4),
            "-m".into(),
            "comment".into(),
            "--comment".into(),
            comment.into(),
            "-j".into(),
            "MASQUERADE".into(),
        ],
    )
}

fn expected_policy_rules(
    state: &VmNetworkStateRecord,
    chain: &str,
    comment: &str,
) -> Vec<PlannedRule> {
    let mut rules = ensure_filter_chain_rules(state, chain, comment);
    rules.extend(ensure_forwarding_entry_rules(state, chain, comment));
    rules.push(ensure_nat_masquerade_rule(state, comment));
    rules
}

fn restore_missing_policy_rules(
    ops: &mut impl PolicyOps,
    expected: &[PlannedRule],
) -> Result<(), NetError> {
    let mut installed_by_chain = HashMap::<(&'static str, String), Option<Vec<Vec<String>>>>::new();
    let mut missing = Vec::new();
    for rule in expected {
        let key = (rule.table, rule.chain.clone());
        let installed = match installed_by_chain.entry(key) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(list_iptables_rule_specs(ops, rule.table, &rule.chain)?)
            }
        };
        if !rule_exists_in_specs(installed.as_deref(), rule) {
            missing.push(rule.clone());
        }
    }
    if missing.is_empty() {
        return Ok(());
    }

    let restore = build_iptables_restore_input(&missing);
    ops.run_command_input(
        "iptables-restore",
        &["-w".to_owned(), "--noflush".to_owned()],
        &restore,
    )
}

fn rule_exists_in_specs(installed: Option<&[Vec<String>]>, rule: &PlannedRule) -> bool {
    installed.is_some_and(|specs| specs.iter().any(|spec| spec == &rule.spec))
}

fn list_iptables_rule_specs(
    ops: &mut impl PolicyOps,
    table: &str,
    chain: &str,
) -> Result<Option<Vec<Vec<String>>>, NetError> {
    let output = ops.command_output("iptables", &iptables_args(table, "-S", chain, &[]))?;
    if !output.status_success {
        return Ok(None);
    }

    let mut rules = Vec::new();
    for line in output.stdout.lines().map(str::trim) {
        if line.is_empty()
            || line == format!("-N {chain}")
            || line.starts_with(&format!("-P {chain} "))
        {
            continue;
        }
        let Some(rest) = line.strip_prefix(&format!("-A {chain} ")) else {
            return Err(NetError::NetworkAllocationConflict {
                path: iptables_state_path(table, chain),
                detail: format!("unexpected iptables-save rule shape {line:?}"),
            });
        };
        rules.push(split_iptables_rule_spec(rest));
    }
    Ok(Some(rules))
}

fn build_iptables_restore_input(rules: &[PlannedRule]) -> String {
    let mut input = String::new();
    for table in ["filter", "nat"] {
        let table_rules = rules
            .iter()
            .filter(|rule| rule.table == table)
            .collect::<Vec<_>>();
        if table_rules.is_empty() {
            continue;
        }
        input.push('*');
        input.push_str(table);
        input.push('\n');
        for rule in table_rules {
            input.push_str(&rule.restore_line());
            input.push('\n');
        }
        input.push_str("COMMIT\n");
    }
    input
}

pub(super) fn split_iptables_rule_spec(rule: &str) -> Vec<String> {
    rule.split_whitespace()
        .map(|arg| arg.trim_matches('"').to_owned())
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlannedRule {
    table: &'static str,
    chain: String,
    spec: Vec<String>,
    insert: bool,
}

impl PlannedRule {
    fn append(table: &'static str, chain: &str, spec: Vec<String>) -> Self {
        Self {
            table,
            chain: chain.to_owned(),
            spec,
            insert: false,
        }
    }

    fn insert(table: &'static str, chain: &str, spec: Vec<String>) -> Self {
        Self {
            table,
            chain: chain.to_owned(),
            spec,
            insert: true,
        }
    }

    fn restore_line(&self) -> String {
        let operation = if self.insert { "-I" } else { "-A" };
        let mut parts = vec![operation.to_owned(), self.chain.clone()];
        if self.insert {
            parts.push("1".to_owned());
        }
        parts.extend(self.spec.iter().cloned());
        parts.join(" ")
    }
}

pub(crate) fn iptables_args(
    table: &str,
    operation: &str,
    chain: &str,
    rest: &[String],
) -> Vec<String> {
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

fn digest_path(path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.as_os_str().as_bytes());
    hex::encode(hasher.finalize())
}

pub(crate) fn invalid_state(path: impl Into<PathBuf>, detail: impl Into<String>) -> NetError {
    NetError::InvalidNetworkState {
        path: path.into(),
        detail: detail.into(),
    }
}

pub(crate) fn iptables_state_path(table: &str, chain: &str) -> PathBuf {
    PathBuf::from(format!("iptables:{table}:{chain}"))
}
