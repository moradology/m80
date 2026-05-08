//! Deliberately malicious attack payloads for m80 jailer tests.

mod attacks;
mod catalog;

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

    /// Return the human-readable block reason.
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
    std::env::var("M80_ATTACK_HOST_SENTINEL")
        .unwrap_or_else(|_| "/m80-host-sentinel-deny".to_owned())
}

pub(crate) fn peer_sentinel() -> String {
    std::env::var("M80_ATTACK_PEER_SENTINEL")
        .unwrap_or_else(|_| "/m80-peer-sentinel-deny".to_owned())
}

pub(crate) fn lower_sentinel() -> String {
    std::env::var("M80_ATTACK_LOWER_SENTINEL").unwrap_or_else(|_| "/lower".to_owned())
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
        let err = run_attack("missing_attack").unwrap_err();
        assert!(err.reason().contains("unknown attack"), "{err}");
    }

    #[test]
    fn echo_zero_is_available_for_harness_negative_control() {
        run_attack("echo_zero").unwrap();
    }
}
