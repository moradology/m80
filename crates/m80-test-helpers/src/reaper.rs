//! Pure helpers for privileged E2E stale-state reaping.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const PROTECTED_RUN_DIR_NAMES: &[&str] = &[".preserved", "warm", "templates"];

/// An iptables rule owned by m80 and safe for the E2E reaper to delete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IptablesDeleteRule {
    /// The iptables table containing the rule.
    pub table: String,
    /// The chain containing the rule.
    pub chain: String,
    /// Arguments after `-D <chain>`.
    pub spec: Vec<String>,
}

/// Return true when a host link name matches an m80-owned transient interface.
#[must_use]
pub fn is_owned_link_name(name: &str) -> bool {
    is_hex_suffix(name, "tfc", 12)
        || is_hex_suffix(name, "brfc", 11)
        || is_hex_suffix(name, "bfc", 12)
        || name.starts_with("m80-br")
}

/// Extract m80-owned link names from `ip -o link show`.
#[must_use]
pub fn owned_link_names(ip_link_show: &str) -> Vec<String> {
    ip_link_show
        .lines()
        .filter_map(link_name_from_ip_line)
        .filter(|name| is_owned_link_name(name))
        .collect()
}

/// Return true when an iptables chain name matches m80's per-VM chain shape.
#[must_use]
pub fn is_owned_iptables_chain(name: &str) -> bool {
    is_hex_suffix(name, "tfw", 12)
}

/// Extract m80-owned chain names from `iptables -S`.
#[must_use]
pub fn owned_iptables_chains(iptables_save: &str) -> Vec<String> {
    iptables_save
        .lines()
        .filter_map(|line| {
            let args = split_iptables_line(line);
            match args.as_slice() {
                [op, chain] if op == "-N" && is_owned_iptables_chain(chain) => {
                    Some(chain.to_owned())
                }
                _ => None,
            }
        })
        .collect()
}

/// Extract m80-comment-owned rules from `iptables -S`.
#[must_use]
pub fn owned_iptables_delete_rules(table: &str, iptables_save: &str) -> Vec<IptablesDeleteRule> {
    iptables_save
        .lines()
        .filter_map(|line| {
            let args = split_iptables_line(line);
            let [op, chain, spec @ ..] = args.as_slice() else {
                return None;
            };
            if op != "-A" || !spec_has_m80_comment(spec) {
                return None;
            }
            Some(IptablesDeleteRule {
                table: table.to_owned(),
                chain: chain.to_owned(),
                spec: spec.to_vec(),
            })
        })
        .collect()
}

/// Return true when a run-dir is old enough and has no live pid file.
#[must_use]
pub fn is_stale_run_dir(path: &Path, now: SystemTime, min_age: Duration) -> bool {
    if !path.is_dir()
        || path.file_name().is_some_and(|name| {
            PROTECTED_RUN_DIR_NAMES
                .iter()
                .any(|protected| name == *protected)
        })
    {
        return false;
    }
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    let Ok(modified) = metadata.modified() else {
        return false;
    };
    let Ok(age) = now.duration_since(modified) else {
        return false;
    };
    age >= min_age && !run_dir_has_live_pid(path)
}

fn is_hex_suffix(value: &str, prefix: &str, hex_len: usize) -> bool {
    let Some(rest) = value.strip_prefix(prefix) else {
        return false;
    };
    rest.len() == hex_len && rest.bytes().all(|b| b.is_ascii_hexdigit())
}

fn link_name_from_ip_line(line: &str) -> Option<String> {
    let (_, rest) = line.split_once(": ")?;
    let raw = rest.split(':').next()?.split('@').next()?.trim();
    if raw.is_empty() {
        return None;
    }
    Some(raw.to_owned())
}

fn split_iptables_line(line: &str) -> Vec<String> {
    line.split_whitespace()
        .map(|part| part.trim_matches('"').to_owned())
        .collect()
}

fn spec_has_m80_comment(spec: &[String]) -> bool {
    spec.windows(2)
        .any(|pair| pair[0] == "--comment" && pair[1].starts_with("m80:"))
}

fn run_dir_has_live_pid(path: &Path) -> bool {
    for pid_path in pid_files(path) {
        let Ok(text) = std::fs::read_to_string(&pid_path) else {
            continue;
        };
        let Ok(pid) = text.trim().parse::<u32>() else {
            continue;
        };
        if PathBuf::from("/proc").join(pid.to_string()).exists() {
            return true;
        }
    }
    false
}

fn pid_files(path: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_pid_files(path, &mut files);
    files
}

fn collect_pid_files(path: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let child = entry.path();
        if child.is_dir() {
            collect_pid_files(&child, files);
        } else if child.extension().is_some_and(|ext| ext == "pid") {
            files.push(child);
        }
    }
}
