//! Deliberately malicious attack payloads for m80 jailer tests.

mod attacks;
mod catalog;
mod config;

use std::fmt;

pub use catalog::{attack_names, attacks_by_category, run_attack, Attack, AttackCategory};

/// Result returned by one attack primitive.
pub type AttackResult = Result<(), AttackBlocked>;

/// Evidence that an attack was blocked by the jail or kernel policy.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{reason}")]
pub struct AttackBlocked {
    reason: String,
}

impl AttackBlocked {
    /// Build a block reason from displayable context.
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    /// Human-readable reason the attack was blocked.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl From<std::io::Error> for AttackBlocked {
    fn from(value: std::io::Error) -> Self {
        Self::new(value.to_string())
    }
}

impl From<nix::Error> for AttackBlocked {
    fn from(value: nix::Error) -> Self {
        Self::new(value.to_string())
    }
}

pub(crate) fn blocked(context: impl fmt::Display, err: impl fmt::Display) -> AttackBlocked {
    AttackBlocked::new(format!("{context}: {err}"))
}

pub(crate) fn host_sentinel() -> String {
    config::value(
        "host_sentinel",
        "M80_ATTACK_HOST_SENTINEL",
        "/m80-host-sentinel-deny",
    )
}

pub(crate) fn peer_sentinel() -> String {
    config::value(
        "peer_sentinel",
        "M80_ATTACK_PEER_SENTINEL",
        "/m80-peer-sentinel-deny",
    )
}

pub(crate) fn lower_sentinel() -> String {
    config::value("lower_sentinel", "M80_ATTACK_LOWER_SENTINEL", "/lower")
}

pub(crate) fn peer_run_dir() -> String {
    config::value("peer_run_dir", "M80_ATTACK_PEER_RUN_DIR", "/run/m80/peer")
}

pub(crate) fn peer_network_state() -> String {
    config::value(
        "peer_network_state",
        "M80_ATTACK_PEER_NETWORK_STATE",
        "/run/m80/peer/network-state.json",
    )
}

pub(crate) fn peer_pid() -> Result<u32, AttackBlocked> {
    let Some(raw_pid) = config::optional_value("peer_pid", "M80_ATTACK_PEER_PID") else {
        return Err(AttackBlocked::new("missing attack config key peer_pid"));
    };
    raw_pid
        .parse::<u32>()
        .map_err(|err| AttackBlocked::new(format!("invalid peer_pid {raw_pid}: {err}")))
}

pub(crate) fn require_peer_config() -> AttackResult {
    for key in ["peer_sentinel", "peer_run_dir", "peer_network_state"] {
        if config::optional_value(key, "").is_none() {
            return Err(AttackBlocked::new(format!(
                "missing attack config key {key}"
            )));
        }
    }
    peer_pid().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_at_least_thirty_real_attacks() {
        let names = attack_names();
        assert!(
            names.len() >= 30,
            "expected at least 30 attacks, got {}",
            names.len()
        );
        assert!(!names.contains(&"echo_zero"));
    }

    #[test]
    fn attacks_are_grouped_by_six_layer_two_categories() {
        let grouped = attacks_by_category();
        assert_eq!(grouped.len(), 6);
        assert!(grouped.iter().all(|(_, attacks)| !attacks.is_empty()));
    }

    #[test]
    fn unknown_attack_is_reported_as_blocked() {
        let err = run_attack("missing_attack").expect_err("unknown attack should fail closed");
        assert!(err.reason().contains("unknown attack"), "{err}");
    }

    #[test]
    fn echo_zero_is_available_for_harness_negative_control() {
        run_attack("echo_zero").expect("echo_zero harness control should succeed");
    }
}
