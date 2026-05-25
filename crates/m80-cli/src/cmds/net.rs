use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::os::unix::ffi::OsStrExt as _;
use std::path::Path;

use m80_firecracker::{ConfigError, FcError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::args::{NetAction, NetCleanupArgs};
use crate::{config, errors, json};

use self::host::{command_failed, HostCommands, RealHostCommands};

const NETWORK_STATE_FILE: &str = "network-state.json";
const RULE_COMMENT_PREFIX: &str = "m80";
const UNKNOWN_VM_ID: &str = "unknown";

pub(super) fn cmd_net(action: NetAction, json_output: bool) -> anyhow::Result<i32> {
    match action {
        NetAction::Cleanup(args) => cmd_net_cleanup(args, json_output),
    }
}

fn cmd_net_cleanup(args: NetCleanupArgs, json_output: bool) -> anyhow::Result<i32> {
    let run_root = match config::resolve_run_root() {
        Ok(path) => path,
        Err(err) => return Ok(errors::render_error(&err, json_output)),
    };
    let mut commands = RealHostCommands;
    let report = match build_net_cleanup_report(&run_root, args.dry_run, &mut commands) {
        Ok(report) => report,
        Err(err) => return Ok(errors::render_error(&err, json_output)),
    };

    if json_output {
        println!("{}", json::to_pretty(&report));
    } else {
        print!("{}", render_net_cleanup_table(&report));
    }
    Ok(0)
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct NetCleanupReport {
    status: &'static str,
    dry_run: bool,
    run_root: String,
    resources: Vec<NetCleanupRow>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct NetCleanupRow {
    orphan_vm_id: String,
    resource_kind: &'static str,
    resource_id: String,
    rule_text: String,
    action: CleanupAction,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum CleanupAction {
    Kept,
    WouldRemove,
    Removed,
}

fn build_net_cleanup_report(
    run_root: &Path,
    dry_run: bool,
    commands: &mut impl HostCommands,
) -> Result<NetCleanupReport, FcError> {
    let inventory = NetworkInventory::scan(run_root)?;
    let run_root_digest = run_root_digest_prefix(run_root);
    let mut rows = Vec::new();
    let mut orphan_filter_chains = BTreeSet::new();
    let mut owned_orphan_taps = BTreeSet::new();

    for table in ["filter", "nat"] {
        for rule in scan_iptables_rules(commands, table)? {
            let Some(comment) = parse_rule_comment(&rule) else {
                continue;
            };
            let vm_id = inventory
                .candidate_vm_id_for_tap(&comment.tap_name)
                .unwrap_or(UNKNOWN_VM_ID)
                .to_owned();
            let orphaned = comment.run_root_digest == run_root_digest
                && !inventory.tap_has_state(&comment.tap_name);
            let mut action = if orphaned {
                CleanupAction::WouldRemove
            } else {
                CleanupAction::Kept
            };
            if orphaned {
                owned_orphan_taps.insert(comment.tap_name.clone());
            }
            if orphaned && rule.table == "filter" {
                if is_m80_filter_chain(&rule.chain) {
                    orphan_filter_chains.insert(rule.chain.clone());
                }
                if let Some(chain) = rule
                    .jump_target
                    .as_deref()
                    .filter(|chain| is_m80_filter_chain(chain))
                {
                    orphan_filter_chains.insert(chain.to_owned());
                }
            }
            if orphaned && !dry_run {
                delete_iptables_rule(commands, &rule)?;
                action = CleanupAction::Removed;
            }
            rows.push(NetCleanupRow {
                orphan_vm_id: vm_id,
                resource_kind: "iptables",
                resource_id: format!("{}:{}", rule.table, rule.chain),
                rule_text: rule.text,
                action,
            });
        }
    }

    for chain in orphan_filter_chains {
        let mut action = CleanupAction::WouldRemove;
        if !dry_run {
            delete_iptables_chain(commands, &chain)?;
            action = CleanupAction::Removed;
        }
        rows.push(NetCleanupRow {
            orphan_vm_id: UNKNOWN_VM_ID.to_owned(),
            resource_kind: "iptables_chain",
            resource_id: format!("filter:{chain}"),
            rule_text: format!("-X {chain}"),
            action,
        });
    }

    for tap in scan_tap_links(commands)? {
        let vm_id = inventory
            .candidate_vm_id_for_tap(&tap.name)
            .unwrap_or(UNKNOWN_VM_ID)
            .to_owned();
        let state_backed = inventory.tap_has_state(&tap.name);
        let orphaned = !state_backed
            && (inventory.tap_is_candidate(&tap.name) || owned_orphan_taps.contains(&tap.name));
        let mut action = if orphaned {
            CleanupAction::WouldRemove
        } else {
            CleanupAction::Kept
        };
        if action == CleanupAction::WouldRemove && !dry_run {
            commands.run("ip", &["link", "delete", &tap.name])?;
            action = CleanupAction::Removed;
        }
        rows.push(NetCleanupRow {
            orphan_vm_id: vm_id,
            resource_kind: "tap",
            resource_id: tap.name,
            rule_text: tap.text,
            action,
        });
    }

    Ok(NetCleanupReport {
        status: "ok",
        dry_run,
        run_root: run_root.display().to_string(),
        resources: rows,
    })
}

fn render_net_cleanup_table(report: &NetCleanupReport) -> String {
    let mut out = String::new();
    writeln!(out, "run_root: {}", report.run_root).unwrap();
    writeln!(
        out,
        "{:<18} {:<15} {:<14} TEXT",
        "ORPHAN_VM_ID", "RESOURCE", "ACTION"
    )
    .unwrap();
    writeln!(out, "{}", "-".repeat(80)).unwrap();
    for row in &report.resources {
        writeln!(
            out,
            "{:<18} {:<15} {:<14} {}",
            row.orphan_vm_id,
            row.resource_kind,
            action_label(row.action),
            row.rule_text
        )
        .unwrap();
    }
    if report.resources.is_empty() {
        writeln!(out, "(no m80 network residue found)").unwrap();
    }
    out
}

fn action_label(action: CleanupAction) -> &'static str {
    match action {
        CleanupAction::Kept => "kept",
        CleanupAction::WouldRemove => "would_remove",
        CleanupAction::Removed => "removed",
    }
}

#[derive(Debug, Default)]
struct NetworkInventory {
    state_taps: BTreeMap<String, String>,
    candidate_taps: BTreeMap<String, String>,
}

impl NetworkInventory {
    fn scan(run_root: &Path) -> Result<Self, FcError> {
        let mut inventory = Self::default();
        if !run_root.exists() {
            return Ok(inventory);
        }
        let entries = fs::read_dir(run_root).map_err(|source| FcError::PathIo {
            path: run_root.to_path_buf(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| FcError::PathIo {
                path: run_root.to_path_buf(),
                source,
            })?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(vm_id) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if vm_id.starts_with('.') || vm_id == "warm" {
                continue;
            }
            let tap_name = derive_tap_name(run_root, vm_id);
            inventory
                .candidate_taps
                .insert(tap_name.clone(), vm_id.to_owned());
            let state_path = path.join(NETWORK_STATE_FILE);
            if state_path.exists() {
                let state = read_network_state_projection(&state_path)?;
                inventory.state_taps.insert(state.tap_name, state.vm_id);
            }
        }
        Ok(inventory)
    }

    fn tap_has_state(&self, tap_name: &str) -> bool {
        self.state_taps.contains_key(tap_name)
    }

    fn tap_is_candidate(&self, tap_name: &str) -> bool {
        self.candidate_taps.contains_key(tap_name)
    }

    fn candidate_vm_id_for_tap(&self, tap_name: &str) -> Option<&str> {
        self.state_taps
            .get(tap_name)
            .or_else(|| self.candidate_taps.get(tap_name))
            .map(String::as_str)
    }
}

#[derive(Debug, Deserialize)]
struct NetworkStateProjection {
    vm_id: String,
    tap_name: String,
}

fn read_network_state_projection(path: &Path) -> Result<NetworkStateProjection, FcError> {
    let bytes = fs::read(path).map_err(|source| FcError::PathIo {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| {
        FcError::Config(ConfigError::InvalidValue {
            field: "network-state.json",
            reason: format!("{}: {source}", path.display()),
        })
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IptablesRule {
    table: &'static str,
    chain: String,
    spec: Vec<String>,
    text: String,
    jump_target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuleComment {
    run_root_digest: String,
    tap_name: String,
}

fn scan_iptables_rules(
    commands: &mut impl HostCommands,
    table: &'static str,
) -> Result<Vec<IptablesRule>, FcError> {
    let args = ["-w", "-t", table, "-S"];
    let output = commands.output("iptables", &args)?;
    if !output.status_success {
        return Err(command_failed("iptables", &output.stderr));
    }
    Ok(output
        .stdout
        .lines()
        .filter_map(|line| parse_iptables_rule(table, line))
        .collect())
}

fn parse_iptables_rule(table: &'static str, line: &str) -> Option<IptablesRule> {
    if !line.contains(RULE_COMMENT_PREFIX) {
        return None;
    }
    let tokens = split_iptables_line(line);
    if tokens.len() < 3 || tokens.first()? != "-A" {
        return None;
    }
    let chain = tokens[1].clone();
    let spec = tokens[2..].to_vec();
    let jump_target = spec
        .windows(2)
        .find(|pair| pair[0] == "-j")
        .map(|pair| pair[1].clone());
    Some(IptablesRule {
        table,
        chain,
        spec,
        text: line.to_owned(),
        jump_target,
    })
}

fn parse_rule_comment(rule: &IptablesRule) -> Option<RuleComment> {
    let comment = rule
        .spec
        .windows(2)
        .find(|pair| pair[0] == "--comment")
        .map(|pair| strip_simple_quotes(&pair[1]))?;
    let mut parts = comment.split(':');
    let prefix = parts.next()?;
    let run_root_digest = parts.next()?;
    let tap_name = parts.next()?;
    if prefix != RULE_COMMENT_PREFIX || parts.next().is_some() || !is_m80_tap_name(tap_name) {
        return None;
    }
    Some(RuleComment {
        run_root_digest: run_root_digest.to_owned(),
        tap_name: tap_name.to_owned(),
    })
}

fn split_iptables_line(line: &str) -> Vec<String> {
    line.split_whitespace().map(strip_simple_quotes).collect()
}

fn strip_simple_quotes(value: &str) -> String {
    value.trim_matches('"').trim_matches('\'').to_owned()
}

fn delete_iptables_rule(
    commands: &mut impl HostCommands,
    rule: &IptablesRule,
) -> Result<(), FcError> {
    let mut args = vec!["-w", "-t", rule.table, "-D", rule.chain.as_str()];
    let specs = rule.spec.iter().map(String::as_str).collect::<Vec<_>>();
    args.extend(specs);
    commands.run("iptables", &args)
}

fn delete_iptables_chain(commands: &mut impl HostCommands, chain: &str) -> Result<(), FcError> {
    commands.run("iptables", &["-w", "-t", "filter", "-X", chain])
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TapLink {
    name: String,
    text: String,
}

fn scan_tap_links(commands: &mut impl HostCommands) -> Result<Vec<TapLink>, FcError> {
    let output = commands.output("ip", &["-o", "link", "show"])?;
    if !output.status_success {
        return Err(command_failed("ip", &output.stderr));
    }
    Ok(output.stdout.lines().filter_map(parse_tap_link).collect())
}

fn parse_tap_link(line: &str) -> Option<TapLink> {
    let first = line.split(':').next()?.trim();
    let name = if is_m80_tap_name(first) {
        first
    } else if let Some((_, rest)) = line.split_once(": ") {
        rest.split(':').next()?
    } else {
        first
    };
    let name = name.split('@').next()?.trim();
    if !is_m80_tap_name(name) {
        return None;
    }
    Some(TapLink {
        name: name.to_owned(),
        text: line.to_owned(),
    })
}

fn is_m80_tap_name(name: &str) -> bool {
    name.len() == 15
        && name.starts_with("tfc")
        && name[3..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_m80_filter_chain(name: &str) -> bool {
    name.len() == 15
        && name.starts_with("tfw")
        && name[3..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn derive_tap_name(run_root: &Path, vm_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(run_root.as_os_str().as_bytes());
    hasher.update(vm_id.as_bytes());
    let digest = hasher.finalize();
    format!("tfc{}", first_hex_chars(&digest, 12))
}

fn run_root_digest_prefix(run_root: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(run_root.as_os_str().as_bytes());
    let digest = hasher.finalize();
    first_hex_chars(&digest, 12)
}

fn first_hex_chars(bytes: &[u8], chars: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(chars);
    for byte in bytes {
        if out.len() == chars {
            break;
        }
        out.push(HEX[(byte >> 4) as usize] as char);
        if out.len() == chars {
            break;
        }
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests;

mod host;
