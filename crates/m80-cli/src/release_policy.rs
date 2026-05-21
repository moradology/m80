//! Release transition policy for installed-version movement.

#![cfg_attr(not(test), allow(dead_code))]

use std::cmp::Ordering;

const AUTOMATIC_UPDATE_ORDERING: &str =
    "target stable vMAJOR.MINOR.PATCH must be newer than or equal to active stable vMAJOR.MINOR.PATCH; older targets require explicit rollback";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StableReleaseTag {
    raw: String,
    major: u64,
    minor: u64,
    patch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReleaseTagError {
    MissingPrefix,
    WrongPartCount,
    EmptyPart { index: usize },
    NonDigitPart { index: usize },
    NumericOverflow { index: usize },
    Prerelease,
    BuildMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReleaseIdentity {
    Stable(StableReleaseTag),
    Prerelease { tag: String },
    BuildMetadata { tag: String },
    Malformed { tag: String },
    LocalDev,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReleaseTransitionState {
    UpgradeAllowed,
    AlreadyCurrent,
    DowngradeRefused,
    ActivePrerelease,
    TargetPrerelease,
    ActiveBuildMetadata,
    TargetBuildMetadata,
    ActiveMalformed,
    TargetMalformed,
    ActiveLocalDev,
}

impl ReleaseTransitionState {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::UpgradeAllowed => "upgrade_allowed",
            Self::AlreadyCurrent => "already_current",
            Self::DowngradeRefused => "downgrade_refused",
            Self::ActivePrerelease => "active_prerelease",
            Self::TargetPrerelease => "target_prerelease",
            Self::ActiveBuildMetadata => "active_build_metadata",
            Self::TargetBuildMetadata => "target_build_metadata",
            Self::ActiveMalformed => "active_malformed",
            Self::TargetMalformed => "target_malformed",
            Self::ActiveLocalDev => "active_local_dev",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReleaseTransitionReport {
    pub(crate) state: ReleaseTransitionState,
    pub(crate) active_tag: Option<String>,
    pub(crate) target_tag: Option<String>,
    pub(crate) expected_ordering: &'static str,
    pub(crate) observed_ordering: &'static str,
    pub(crate) diagnostic: String,
}

pub(crate) fn parse_stable_release_tag(tag: &str) -> Result<StableReleaseTag, ReleaseTagError> {
    let Some(version) = tag.strip_prefix('v') else {
        return Err(ReleaseTagError::MissingPrefix);
    };
    if version.contains('-') {
        return Err(ReleaseTagError::Prerelease);
    }
    if version.contains('+') {
        return Err(ReleaseTagError::BuildMetadata);
    }

    let parts = version.split('.').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err(ReleaseTagError::WrongPartCount);
    }
    let major = parse_numeric_part(parts[0], 0)?;
    let minor = parse_numeric_part(parts[1], 1)?;
    let patch = parse_numeric_part(parts[2], 2)?;
    Ok(StableReleaseTag {
        raw: tag.to_owned(),
        major,
        minor,
        patch,
    })
}

pub(crate) fn is_stable_release_tag(tag: &str) -> bool {
    parse_stable_release_tag(tag).is_ok()
}

pub(crate) fn classify_release_tag(tag: &str) -> ReleaseIdentity {
    match parse_stable_release_tag(tag) {
        Ok(stable) => ReleaseIdentity::Stable(stable),
        Err(ReleaseTagError::Prerelease) => ReleaseIdentity::Prerelease {
            tag: tag.to_owned(),
        },
        Err(ReleaseTagError::BuildMetadata) => ReleaseIdentity::BuildMetadata {
            tag: tag.to_owned(),
        },
        Err(_) => ReleaseIdentity::Malformed {
            tag: tag.to_owned(),
        },
    }
}

pub(crate) fn local_dev_release_identity() -> ReleaseIdentity {
    ReleaseIdentity::LocalDev
}

pub(crate) fn release_transition(
    active: ReleaseIdentity,
    target: ReleaseIdentity,
) -> ReleaseTransitionReport {
    let state = transition_state(&active, &target);
    let observed_ordering = observed_ordering_label(state);
    let active_tag = identity_tag(&active).map(str::to_owned);
    let target_tag = identity_tag(&target).map(str::to_owned);
    let diagnostic = format!(
        "release transition {}: expected {}; observed active={} target={} ordering={}",
        state.as_str(),
        AUTOMATIC_UPDATE_ORDERING,
        identity_label(active_tag.as_deref()),
        identity_label(target_tag.as_deref()),
        observed_ordering,
    );
    ReleaseTransitionReport {
        state,
        active_tag,
        target_tag,
        expected_ordering: AUTOMATIC_UPDATE_ORDERING,
        observed_ordering,
        diagnostic,
    }
}

impl StableReleaseTag {
    pub(crate) fn as_str(&self) -> &str {
        &self.raw
    }
}

impl Ord for StableReleaseTag {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch))
    }
}

impl PartialOrd for StableReleaseTag {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn parse_numeric_part(part: &str, index: usize) -> Result<u64, ReleaseTagError> {
    if part.is_empty() {
        return Err(ReleaseTagError::EmptyPart { index });
    }
    if !part.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ReleaseTagError::NonDigitPart { index });
    }
    part.parse::<u64>()
        .map_err(|_source| ReleaseTagError::NumericOverflow { index })
}

fn transition_state(active: &ReleaseIdentity, target: &ReleaseIdentity) -> ReleaseTransitionState {
    match active {
        ReleaseIdentity::LocalDev => return ReleaseTransitionState::ActiveLocalDev,
        ReleaseIdentity::Prerelease { .. } => return ReleaseTransitionState::ActivePrerelease,
        ReleaseIdentity::BuildMetadata { .. } => {
            return ReleaseTransitionState::ActiveBuildMetadata;
        }
        ReleaseIdentity::Malformed { .. } => return ReleaseTransitionState::ActiveMalformed,
        ReleaseIdentity::Stable(_) => {}
    }
    match target {
        ReleaseIdentity::Prerelease { .. } => return ReleaseTransitionState::TargetPrerelease,
        ReleaseIdentity::BuildMetadata { .. } => {
            return ReleaseTransitionState::TargetBuildMetadata;
        }
        ReleaseIdentity::Malformed { .. } | ReleaseIdentity::LocalDev => {
            return ReleaseTransitionState::TargetMalformed;
        }
        ReleaseIdentity::Stable(_) => {}
    }
    let (ReleaseIdentity::Stable(active), ReleaseIdentity::Stable(target)) = (active, target)
    else {
        unreachable!("non-stable identities returned above");
    };
    match target.cmp(active) {
        Ordering::Greater => ReleaseTransitionState::UpgradeAllowed,
        Ordering::Equal => ReleaseTransitionState::AlreadyCurrent,
        Ordering::Less => ReleaseTransitionState::DowngradeRefused,
    }
}

fn observed_ordering_label(state: ReleaseTransitionState) -> &'static str {
    match state {
        ReleaseTransitionState::UpgradeAllowed => "target_newer",
        ReleaseTransitionState::AlreadyCurrent => "target_same",
        ReleaseTransitionState::DowngradeRefused => "target_older",
        ReleaseTransitionState::ActivePrerelease => "active_prerelease",
        ReleaseTransitionState::TargetPrerelease => "target_prerelease",
        ReleaseTransitionState::ActiveBuildMetadata => "active_build_metadata",
        ReleaseTransitionState::TargetBuildMetadata => "target_build_metadata",
        ReleaseTransitionState::ActiveMalformed => "active_malformed",
        ReleaseTransitionState::TargetMalformed => "target_malformed",
        ReleaseTransitionState::ActiveLocalDev => "active_local_dev",
    }
}

fn identity_tag(identity: &ReleaseIdentity) -> Option<&str> {
    match identity {
        ReleaseIdentity::Stable(tag) => Some(tag.as_str()),
        ReleaseIdentity::Prerelease { tag }
        | ReleaseIdentity::BuildMetadata { tag }
        | ReleaseIdentity::Malformed { tag } => Some(tag),
        ReleaseIdentity::LocalDev => None,
    }
}

fn identity_label(tag: Option<&str>) -> &str {
    tag.unwrap_or("<local-dev>")
}

#[cfg(test)]
mod tests;
