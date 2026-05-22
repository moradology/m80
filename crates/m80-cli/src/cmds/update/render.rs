use crate::install_state::InstallStateKind;

use super::{SafetyFloorStatus, UpdateCheckOutput, UpdateCheckState, UpdateProofCacheStatus};

pub(super) fn render_human(output: &UpdateCheckOutput) -> String {
    let mut text = String::new();
    push_line(&mut text, "update_check_state", output.state.as_str());
    push_line(
        &mut text,
        "install_status",
        install_state_label(output.install_status),
    );
    push_optional(&mut text, "active_tag", output.active_tag.as_deref());
    push_optional(
        &mut text,
        "latest_stable_tag",
        output.latest_stable_tag.as_deref(),
    );
    push_line(
        &mut text,
        "latest_status_source",
        &output.latest_status_source,
    );
    push_line(
        &mut text,
        "latest_status_origin",
        output.latest_status_origin.as_str(),
    );
    push_line(
        &mut text,
        "latest_status_cache_state",
        output.latest_status_cache_state.as_str(),
    );
    push_optional_string(
        &mut text,
        "latest_status_fetched_at",
        output
            .latest_status_fetched_at
            .map(|timestamp| timestamp.to_string()),
    );
    push_line(
        &mut text,
        "latest_status_max_age_seconds",
        output.latest_status_max_age_seconds,
    );
    push_optional(
        &mut text,
        "latest_status_error",
        output.latest_status_error.as_deref(),
    );
    push_optional(
        &mut text,
        "latest_status_offline_reason",
        output.latest_status_offline_reason.as_deref(),
    );
    push_line(
        &mut text,
        "safety_floor_status",
        output.safety_floor.status.as_str(),
    );
    push_optional(
        &mut text,
        "safety_floor_minimum_safe_tag",
        output.safety_floor.minimum_safe_tag.as_deref(),
    );
    push_line(
        &mut text,
        "safety_floor_yanked_count",
        output.safety_floor.yanked_tags.len(),
    );
    push_line(
        &mut text,
        "proof_cache_status",
        output.proof_cache_status.as_str(),
    );
    push_optional_string(
        &mut text,
        "proof_cache_age_seconds",
        output.proof_cache_age_seconds.map(|age| age.to_string()),
    );
    push_optional(&mut text, "apply_command", output.apply_command.as_deref());
    push_optional(
        &mut text,
        "reinstall_command",
        output.reinstall_command.as_deref(),
    );
    push_optional(&mut text, "retry_command", output.retry_command.as_deref());
    push_optional(&mut text, "next_command", output.next_command.as_deref());
    push_line(&mut text, "message", &output.message);
    text
}

pub(super) fn update_message(state: UpdateCheckState) -> &'static str {
    match state {
        UpdateCheckState::Current => "active release is current",
        UpdateCheckState::Outdated => "a newer stable release is available",
        UpdateCheckState::Yanked => "active or target release is yanked",
        UpdateCheckState::Unsafe => "active or target release is below the safety floor",
        UpdateCheckState::UnknownOffline => {
            "latest status is unavailable; active freshness is unknown"
        }
        UpdateCheckState::StaleLatestMetadata => {
            "latest status is stale; rerun freshness before applying updates"
        }
        UpdateCheckState::PrereleaseActive => {
            "active release is a prerelease; use explicit install or rollback"
        }
        UpdateCheckState::IneligibleActive => {
            "active release tag is not eligible for automatic update checks"
        }
        UpdateCheckState::LocalDevInstall => {
            "local development profile is selected; no release update applies"
        }
        UpdateCheckState::InstallUnhealthy => {
            "local install state is unhealthy; repair before checking for updates"
        }
    }
}

fn push_line(text: &mut String, key: &str, value: impl std::fmt::Display) {
    text.push_str(key);
    text.push('=');
    text.push_str(&value.to_string());
    text.push('\n');
}

fn push_optional(text: &mut String, key: &str, value: Option<&str>) {
    push_line(text, key, value.unwrap_or("<unavailable>"));
}

fn push_optional_string(text: &mut String, key: &str, value: Option<String>) {
    push_line(text, key, value.as_deref().unwrap_or("<unavailable>"));
}

impl UpdateCheckState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Outdated => "outdated",
            Self::Yanked => "yanked",
            Self::Unsafe => "unsafe",
            Self::UnknownOffline => "unknown_offline",
            Self::StaleLatestMetadata => "stale_latest_metadata",
            Self::PrereleaseActive => "prerelease_active",
            Self::IneligibleActive => "ineligible_active",
            Self::LocalDevInstall => "local_dev_install",
            Self::InstallUnhealthy => "install_unhealthy",
        }
    }
}

impl SafetyFloorStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Safe => "safe",
            Self::ActiveYanked => "active_yanked",
            Self::LatestYanked => "latest_yanked",
            Self::ActiveBelowMinimum => "active_below_minimum",
            Self::LatestBelowMinimum => "latest_below_minimum",
        }
    }
}

impl UpdateProofCacheStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::MissingActiveInstall => "missing_active_install",
            Self::LocalDevInstall => "local_dev_install",
            Self::MissingManifest => "missing_manifest",
            Self::InvalidManifest => "invalid_manifest",
            Self::StaleManifest => "stale_manifest",
            Self::Unavailable => "unavailable",
        }
    }
}

fn install_state_label(state: InstallStateKind) -> &'static str {
    match state {
        InstallStateKind::HealthyActiveRelease => "healthy_active_release",
        InstallStateKind::MissingActivePointer => "missing_active_pointer",
        InstallStateKind::DanglingActivePointer => "dangling_active_pointer",
        InstallStateKind::LocalDevTree => "local_dev_tree",
        InstallStateKind::StaleProfileTarget => "stale_profile_target",
        InstallStateKind::ExplicitOverride => "explicit_override",
        InstallStateKind::MissingInstallMetadata => "missing_install_metadata",
        InstallStateKind::StaleInstallMetadata => "stale_install_metadata",
        InstallStateKind::TamperedProofCache => "tampered_proof_cache",
        InstallStateKind::InvalidInstallMetadata => "invalid_install_metadata",
    }
}
