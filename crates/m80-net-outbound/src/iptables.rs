use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use ipnet::Ipv4Net;
use sha2::{Digest, Sha256};

use crate::{NetError, SetupPhase, VmNetworkStateRecord, M80_RULE_COMMENT_PREFIX};

/// Output returned by a host network policy command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyCommandOutput {
    /// Whether the process exited successfully.
    pub status_success: bool,
    /// Captured stdout as best-effort UTF-8.
    pub stdout: String,
    /// Captured stderr as best-effort UTF-8.
    pub stderr: String,
}

impl PolicyCommandOutput {
    /// Construct a successful command output with stdout.
    pub fn success(stdout: impl Into<String>) -> Self {
        Self {
            status_success: true,
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    /// Construct a failed command output with stderr.
    pub fn failure(stderr: impl Into<String>) -> Self {
        Self {
            status_success: false,
            stdout: String::new(),
            stderr: stderr.into(),
        }
    }
}

/// Host command seam for IPv4 forwarding and iptables policy operations.
pub trait PolicyOps {
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
}

/// Apply the outbound NAT iptables policy with real host commands.
pub fn apply_outbound_nat_policy(state: &VmNetworkStateRecord) -> Result<(), NetError> {
    let mut ops = CommandPolicyOps;
    apply_outbound_nat_policy_with_ops(&mut ops, state)
}

/// Apply the outbound NAT iptables policy through a supplied command backend.
pub fn apply_outbound_nat_policy_with_ops(
    ops: &mut impl PolicyOps,
    state: &VmNetworkStateRecord,
) -> Result<(), NetError> {
    validate_ready_state(state)?;
    let chain = outbound_nat_filter_chain(state);
    let comment = outbound_nat_rule_comment(state);
    ensure_iptables_chain(ops, "filter", &chain)?;
    reject_foreign_iptables_chain_rules(ops, "filter", &chain, &comment)?;
    ensure_ipv4_forwarding(ops)?;
    ensure_filter_chain_rules(ops, state, &chain, &comment)?;
    ensure_forwarding_entry_rules(ops, state, &chain, &comment)?;
    ensure_nat_masquerade_rule(ops, state, &comment)
}

/// Return the per-VM filter chain name.
///
/// Format: `tfw` followed by the first 12 hex chars of `sha256(run_dir)`.
pub fn outbound_nat_filter_chain(state: &VmNetworkStateRecord) -> String {
    format!("tfw{}", &digest_path(&state.run_dir)[..12])
}

/// Return the per-VM iptables rule comment.
///
/// Format: `<M80_RULE_COMMENT_PREFIX>:<first 12 hex chars sha256(run_root)>:<tap_name>`.
pub fn outbound_nat_rule_comment(state: &VmNetworkStateRecord) -> String {
    format!(
        "{}:{}:{}",
        M80_RULE_COMMENT_PREFIX,
        &digest_path(&state.bridge.run_root)[..12],
        state.tap_name
    )
}

/// Return the permanent-deny IPv4 CIDRs for one VM policy.
pub fn permanent_deny_cidrs(bridge_cidr: Ipv4Net) -> Vec<Ipv4Net> {
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

fn ensure_ipv4_forwarding(ops: &mut impl PolicyOps) -> Result<(), NetError> {
    ops.run_command(
        "sysctl",
        &["-w".to_owned(), "net.ipv4.ip_forward=1".to_owned()],
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
    ops: &mut impl PolicyOps,
    state: &VmNetworkStateRecord,
    chain: &str,
    comment: &str,
) -> Result<(), NetError> {
    for resolver in &state.dns_resolvers {
        ensure_iptables_rule(
            ops,
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
            RulePlacement::Append,
        )?;
        ensure_iptables_rule(
            ops,
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
            RulePlacement::Append,
        )?;
    }

    for protocol in ["udp", "tcp"] {
        ensure_iptables_rule(
            ops,
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
            RulePlacement::Append,
        )?;
    }

    for exception in &state.private_ipv4_exceptions {
        ensure_iptables_rule(
            ops,
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
            RulePlacement::Append,
        )?;
    }

    for cidr in permanent_deny_cidrs(state.bridge.cidr) {
        ensure_iptables_rule(
            ops,
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
            RulePlacement::Append,
        )?;
    }

    ensure_iptables_rule(
        ops,
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
        RulePlacement::Append,
    )
}

fn ensure_forwarding_entry_rules(
    ops: &mut impl PolicyOps,
    state: &VmNetworkStateRecord,
    chain: &str,
    comment: &str,
) -> Result<(), NetError> {
    let guest = format!("{}/32", state.guest_ipv4);
    ensure_iptables_rule(
        ops,
        "filter",
        "FORWARD",
        vec![
            "-i".into(),
            state.tap_name.clone(),
            "-s".into(),
            guest.clone(),
            "-m".into(),
            "comment".into(),
            "--comment".into(),
            comment.into(),
            "-j".into(),
            chain.into(),
        ],
        RulePlacement::Insert,
    )?;
    ensure_iptables_rule(
        ops,
        "filter",
        "FORWARD",
        vec![
            "-o".into(),
            state.tap_name.clone(),
            "-d".into(),
            guest.clone(),
            "-m".into(),
            "comment".into(),
            "--comment".into(),
            comment.into(),
            "-j".into(),
            "REJECT".into(),
        ],
        RulePlacement::Insert,
    )?;
    ensure_iptables_rule(
        ops,
        "filter",
        "FORWARD",
        vec![
            "-o".into(),
            state.tap_name.clone(),
            "-d".into(),
            guest,
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
        RulePlacement::Insert,
    )
}

fn ensure_nat_masquerade_rule(
    ops: &mut impl PolicyOps,
    state: &VmNetworkStateRecord,
    comment: &str,
) -> Result<(), NetError> {
    ensure_iptables_rule(
        ops,
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
        RulePlacement::Append,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RulePlacement {
    Append,
    Insert,
}

fn ensure_iptables_rule(
    ops: &mut impl PolicyOps,
    table: &str,
    chain: &str,
    rule: Vec<String>,
    placement: RulePlacement,
) -> Result<(), NetError> {
    if ops
        .command_output("iptables", &iptables_args(table, "-C", chain, &rule))?
        .status_success
    {
        return Ok(());
    }

    let mut args = iptables_args(
        table,
        match placement {
            RulePlacement::Append => "-A",
            RulePlacement::Insert => "-I",
        },
        chain,
        &[],
    );
    if placement == RulePlacement::Insert {
        args.push("1".to_owned());
    }
    args.extend(rule);
    ops.run_command("iptables", &args)
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

fn digest_path(path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.as_os_str().as_bytes());
    hex::encode(hasher.finalize())
}

fn invalid_state(path: impl Into<PathBuf>, detail: impl Into<String>) -> NetError {
    NetError::InvalidNetworkState {
        path: path.into(),
        detail: detail.into(),
    }
}

fn iptables_state_path(table: &str, chain: &str) -> PathBuf {
    PathBuf::from(format!("iptables:{table}:{chain}"))
}
