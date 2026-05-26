//! Pure helpers for privileged E2E post-test leak checks.

use std::collections::BTreeSet;
use std::fmt;
use std::path::PathBuf;

use crate::reaper::{owned_iptables_chains, owned_iptables_delete_rules, owned_link_names};

const PROTECTED_RUN_DIR_NAMES: &[&str] = &[".preserved", "warm", "templates"];

/// A resource snapshot scoped to m80-owned transient E2E residue.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LeakSnapshot {
    /// m80-owned TAP/bridge/link names.
    pub links: BTreeSet<String>,
    /// m80-owned iptables chains by table.
    pub iptables_chains: BTreeSet<IptablesChainLeak>,
    /// m80-comment-owned iptables rules by table/chain/spec.
    pub iptables_rules: BTreeSet<IptablesRuleLeak>,
    /// Direct run-root child directories considered VM residue.
    pub run_dirs: BTreeSet<PathBuf>,
    /// Empty m80-firecracker cgroup leaves.
    pub cgroups: BTreeSet<PathBuf>,
}

impl LeakSnapshot {
    /// Build a snapshot from already-captured host command output.
    #[must_use]
    pub fn from_observations(
        ip_link_show: &str,
        iptables_filter: &str,
        iptables_nat: &str,
        run_root_entries: impl IntoIterator<Item = PathBuf>,
        cgroup_entries: impl IntoIterator<Item = PathBuf>,
    ) -> Self {
        let mut snapshot = Self::default();
        snapshot.links = owned_link_names(ip_link_show).into_iter().collect();
        snapshot.add_iptables_table("filter", iptables_filter);
        snapshot.add_iptables_table("nat", iptables_nat);
        snapshot.run_dirs = run_root_entries
            .into_iter()
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| !PROTECTED_RUN_DIR_NAMES.contains(&name))
            })
            .collect();
        snapshot.cgroups = cgroup_entries.into_iter().collect();
        snapshot
    }

    /// Return m80-owned resources present in `after` but absent in `self`.
    #[must_use]
    pub fn diff_new_resources(&self, after: &Self) -> LeakReport {
        let mut leaks = Vec::new();
        leaks.extend(
            after
                .links
                .difference(&self.links)
                .map(|target| ResourceLeak::new("link", target.clone())),
        );
        leaks.extend(
            after
                .iptables_chains
                .difference(&self.iptables_chains)
                .map(|target| ResourceLeak::new("iptables-chain", target.to_string())),
        );
        leaks.extend(
            after
                .iptables_rules
                .difference(&self.iptables_rules)
                .map(|target| ResourceLeak::new("iptables-rule", target.to_string())),
        );
        leaks.extend(
            after
                .run_dirs
                .difference(&self.run_dirs)
                .map(|target| ResourceLeak::new("run-dir", target.display().to_string())),
        );
        leaks.extend(
            after
                .cgroups
                .difference(&self.cgroups)
                .map(|target| ResourceLeak::new("cgroup", target.display().to_string())),
        );
        LeakReport { leaks }
    }

    fn add_iptables_table(&mut self, table: &str, iptables_save: &str) {
        self.iptables_chains
            .extend(
                owned_iptables_chains(iptables_save)
                    .into_iter()
                    .map(|chain| IptablesChainLeak {
                        table: table.to_owned(),
                        chain,
                    }),
            );
        self.iptables_rules.extend(
            owned_iptables_delete_rules(table, iptables_save)
                .into_iter()
                .map(|rule| IptablesRuleLeak {
                    table: rule.table,
                    chain: rule.chain,
                    spec: rule.spec,
                }),
        );
    }
}

/// An m80-owned iptables chain visible after a test.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct IptablesChainLeak {
    /// iptables table.
    pub table: String,
    /// chain name.
    pub chain: String,
}

impl fmt::Display for IptablesChainLeak {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.table, self.chain)
    }
}

/// An m80-comment-owned iptables rule visible after a test.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct IptablesRuleLeak {
    /// iptables table.
    pub table: String,
    /// chain containing the rule.
    pub chain: String,
    /// delete-rule spec after `-D <chain>`.
    pub spec: Vec<String>,
}

impl fmt::Display for IptablesRuleLeak {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{} {:?}", self.table, self.chain, self.spec)
    }
}

/// A single leaked m80-owned resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceLeak {
    /// Resource kind.
    pub kind: String,
    /// Human-readable target.
    pub target: String,
}

impl ResourceLeak {
    fn new(kind: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            target: target.into(),
        }
    }
}

/// Leak diff between pre-test and post-test snapshots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeakReport {
    /// Newly-visible resources.
    pub leaks: Vec<ResourceLeak>,
}

impl LeakReport {
    /// True when the post-test snapshot added no m80-owned residue.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.leaks.is_empty()
    }
}

impl fmt::Display for LeakReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.leaks.is_empty() {
            return write!(f, "no m80-owned resource leaks");
        }
        writeln!(f, "m80-owned resource leaks:")?;
        for leak in &self.leaks {
            writeln!(f, "- {}: {}", leak.kind, leak.target)?;
        }
        Ok(())
    }
}
