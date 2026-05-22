use std::time::{SystemTime, UNIX_EPOCH};

use m80_firecracker::{ConfigError, FcError};
use serde::Serialize;

use crate::args::UpdateArgs;
use crate::errors;
use crate::install_state::{
    resolve_install_state, InstallStateDiagnostic, InstallStateDiagnosticCode, InstallStateKind,
    InstallStatePaths, InstallStateReport,
};
use crate::json;
use crate::release_freshness::{
    compare_freshness, latest_status_max_age_seconds, ActiveInstallVersion, FreshnessState,
    LatestFreshnessMetadata, LatestMetadata, UnixSeconds,
};
use crate::release_policy::{classify_release_tag, parse_stable_release_tag, ReleaseIdentity};

use active_kind::{active_install_kind, ActiveInstallKind};
use latest_status::{
    latest_status_input, LatestStatusCacheState, LatestStatusInput, LatestStatusOrigin,
};
use render::{render_human, update_message};

pub(super) fn cmd_update(args: UpdateArgs, json_mode: bool) -> anyhow::Result<i32> {
    if !args.check {
        let err = FcError::Config(ConfigError::MissingField {
            field: "update.mode",
        });
        return Ok(errors::render_error(&err, json_mode));
    }

    let paths = install_state_paths(&args);
    let report = resolve_install_state(crate::install_state::InstallStateRequest {
        paths,
        profile_override: args.profile.clone(),
    });
    let latest = if needs_latest_status(&report) {
        match latest_status_input(&args) {
            Ok(latest) => latest,
            Err(err) => return Ok(errors::render_error(&err, json_mode)),
        }
    } else {
        LatestStatusInput::UnknownOffline {
            source: "not-read".to_owned(),
            detail: "local install state does not need latest metadata".to_owned(),
            origin: LatestStatusOrigin::NotRead,
            cache_state: LatestStatusCacheState::NotUsed,
        }
    };
    let output = check_output(&report, latest, current_unix_seconds());

    if json_mode {
        println!("{}", json::to_pretty(&output));
    } else {
        print!("{}", render_human(&output));
    }
    Ok(0)
}

fn needs_latest_status(report: &InstallStateReport) -> bool {
    matches!(report.state, InstallStateKind::HealthyActiveRelease)
}

fn install_state_paths(args: &UpdateArgs) -> InstallStatePaths {
    let mut paths = InstallStatePaths::host(args.install_root.clone());
    if let Some(config_path) = &args.config_path {
        paths.config_paths.system = Some(config_path.clone());
        paths.config_paths.system_drop_in_dir = None;
        paths.config_paths.user = None;
        paths.config_paths.user_drop_in_dir = None;
    }
    if let Some(profile_dir) = &args.profile_dir {
        paths.profile_paths.system_dir = Some(profile_dir.clone());
        paths.profile_paths.user_dir = None;
    }
    paths
}

#[derive(Debug, Clone, Serialize)]
struct UpdateCheckOutput {
    schema_version: u16,
    state: UpdateCheckState,
    freshness_state: UpdateCheckState,
    active_kind: ActiveInstallKind,
    install_status: InstallStateKind,
    active_tag: Option<String>,
    latest_stable_tag: Option<String>,
    latest_status_source: String,
    latest_status_origin: LatestStatusOrigin,
    latest_status_cache_state: LatestStatusCacheState,
    latest_status_fetched_at: Option<i64>,
    latest_status_max_age_seconds: u64,
    latest_status_error: Option<String>,
    latest_status_offline_reason: Option<String>,
    retry_command: Option<String>,
    safety_floor: SafetyFloorOutput,
    proof_cache_status: UpdateProofCacheStatus,
    proof_cache_age_seconds: Option<u64>,
    apply_command: Option<String>,
    reinstall_command: Option<String>,
    next_command: Option<String>,
    message: String,
    diagnostics: Vec<InstallStateDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum UpdateCheckState {
    Current,
    Outdated,
    Yanked,
    Unsafe,
    UnknownOffline,
    StaleLatestMetadata,
    PrereleaseActive,
    IneligibleActive,
    LocalDevInstall,
    InstallUnhealthy,
}

#[derive(Debug, Clone, Serialize)]
struct SafetyFloorOutput {
    status: SafetyFloorStatus,
    minimum_safe_tag: Option<String>,
    yanked_tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum SafetyFloorStatus {
    Unknown,
    Safe,
    ActiveYanked,
    LatestYanked,
    ActiveBelowMinimum,
    LatestBelowMinimum,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum UpdateProofCacheStatus {
    Available,
    MissingActiveInstall,
    LocalDevInstall,
    MissingManifest,
    InvalidManifest,
    StaleManifest,
    Unavailable,
}

fn check_output(
    report: &InstallStateReport,
    latest: LatestStatusInput,
    now: UnixSeconds,
) -> UpdateCheckOutput {
    let active_tag = active_release_tag(report).map(str::to_owned);
    let active_kind = active_install_kind(report, active_tag.as_deref());
    let proof_cache_status = proof_cache_status(report);
    let proof_cache_age_seconds = report
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.proof_cache.as_ref())
        .and_then(|proof_cache| proof_cache.cache_age_seconds);
    let (
        latest_status_source,
        latest_status_origin,
        latest_status_error,
        latest_status_offline_reason,
        latest_status_cache_state,
        latest_metadata,
    ) = match latest {
        LatestStatusInput::Available {
            source,
            metadata,
            origin,
            offline_reason,
        } => {
            let cache_state = if origin == LatestStatusOrigin::CacheFallback {
                if metadata.is_stale_at(now) {
                    LatestStatusCacheState::Stale
                } else {
                    LatestStatusCacheState::Fresh
                }
            } else {
                LatestStatusCacheState::NotUsed
            };
            (
                source,
                origin,
                None,
                offline_reason,
                cache_state,
                Some(metadata),
            )
        }
        LatestStatusInput::UnknownOffline {
            source,
            detail,
            origin,
            cache_state,
        } => (
            source,
            origin,
            Some(detail.clone()),
            Some(detail),
            cache_state,
            None,
        ),
    };
    let latest_stable_tag = latest_metadata
        .as_ref()
        .map(|metadata| metadata.latest_tag().to_owned());
    let latest_status_fetched_at = latest_metadata
        .as_ref()
        .map(|metadata| metadata.published_at().as_i64());
    let latest_status_max_age_seconds = latest_status_max_age_seconds();

    let safety_floor = safety_floor_output(active_tag.as_deref(), latest_metadata.as_ref());
    let state = update_state(report, latest_metadata.as_ref(), safety_floor.status, now);
    let apply_command = apply_command(state, &safety_floor, latest_metadata.as_ref());
    let reinstall_command = reinstall_command_for_state(state, report, active_tag.as_deref());
    let retry_command = matches!(
        state,
        UpdateCheckState::UnknownOffline | UpdateCheckState::StaleLatestMetadata
    )
    .then(|| "m80 update --check".to_owned());
    let next_command = apply_command
        .clone()
        .or_else(|| reinstall_command.clone())
        .or_else(|| retry_command.clone())
        .or_else(|| {
            (state == UpdateCheckState::Current).then(|| "m80 run -- echo hello".to_owned())
        });
    let message = update_message(state);

    UpdateCheckOutput {
        schema_version: 1,
        state,
        freshness_state: state,
        active_kind,
        install_status: report.state,
        active_tag,
        latest_stable_tag,
        latest_status_source,
        latest_status_origin,
        latest_status_cache_state,
        latest_status_fetched_at,
        latest_status_max_age_seconds,
        latest_status_error,
        latest_status_offline_reason,
        retry_command,
        safety_floor,
        proof_cache_status,
        proof_cache_age_seconds,
        apply_command,
        reinstall_command,
        next_command,
        message: message.to_owned(),
        diagnostics: report.diagnostics.clone(),
    }
}

fn update_state(
    report: &InstallStateReport,
    latest: Option<&LatestFreshnessMetadata>,
    safety_status: SafetyFloorStatus,
    now: UnixSeconds,
) -> UpdateCheckState {
    if report.state == InstallStateKind::LocalDevTree {
        return UpdateCheckState::LocalDevInstall;
    }
    if report.state != InstallStateKind::HealthyActiveRelease {
        return UpdateCheckState::InstallUnhealthy;
    }
    if matches!(
        safety_status,
        SafetyFloorStatus::ActiveYanked | SafetyFloorStatus::LatestYanked
    ) {
        return UpdateCheckState::Yanked;
    }
    if matches!(
        safety_status,
        SafetyFloorStatus::ActiveBelowMinimum | SafetyFloorStatus::LatestBelowMinimum
    ) {
        return UpdateCheckState::Unsafe;
    }
    match compare_freshness(active_install_version(report), latest_metadata(latest), now) {
        FreshnessState::Current => UpdateCheckState::Current,
        FreshnessState::Outdated => UpdateCheckState::Outdated,
        FreshnessState::UnknownOffline => UpdateCheckState::UnknownOffline,
        FreshnessState::StaleLatestMetadata => UpdateCheckState::StaleLatestMetadata,
        FreshnessState::PrereleaseActive => UpdateCheckState::PrereleaseActive,
        FreshnessState::IneligibleActive => UpdateCheckState::IneligibleActive,
        FreshnessState::LocalDevActive => UpdateCheckState::LocalDevInstall,
    }
}

fn latest_metadata(latest: Option<&LatestFreshnessMetadata>) -> LatestMetadata<'_> {
    latest.map_or(LatestMetadata::UnknownOffline, LatestMetadata::Available)
}

fn active_install_version(report: &InstallStateReport) -> ActiveInstallVersion<'_> {
    let Some(tag) = active_release_tag(report) else {
        return ActiveInstallVersion::Ineligible { tag: "<missing>" };
    };
    match classify_release_tag(tag) {
        ReleaseIdentity::Stable(_) => ActiveInstallVersion::Release { tag },
        ReleaseIdentity::Prerelease { .. } => ActiveInstallVersion::Prerelease { tag },
        ReleaseIdentity::BuildMetadata { .. } | ReleaseIdentity::Malformed { .. } => {
            ActiveInstallVersion::Ineligible { tag }
        }
        ReleaseIdentity::LocalDev => ActiveInstallVersion::LocalDev,
    }
}

fn active_release_tag(report: &InstallStateReport) -> Option<&str> {
    report.active_pointer.release_tag.as_deref().or_else(|| {
        report
            .profile
            .as_ref()
            .and_then(|profile| profile.release_tag.as_deref())
    })
}

fn safety_floor_output(
    active_tag: Option<&str>,
    latest: Option<&LatestFreshnessMetadata>,
) -> SafetyFloorOutput {
    let Some(latest) = latest else {
        return SafetyFloorOutput {
            status: SafetyFloorStatus::Unknown,
            minimum_safe_tag: None,
            yanked_tags: Vec::new(),
        };
    };
    let floor = latest.safety_floor();
    let status = match active_tag {
        Some(tag) if floor.is_yanked(tag) => SafetyFloorStatus::ActiveYanked,
        _ if floor.is_yanked(latest.latest_tag()) => SafetyFloorStatus::LatestYanked,
        Some(tag) if is_below_minimum_safe(tag, floor.minimum_safe_tag()) => {
            SafetyFloorStatus::ActiveBelowMinimum
        }
        _ if is_below_minimum_safe(latest.latest_tag(), floor.minimum_safe_tag()) => {
            SafetyFloorStatus::LatestBelowMinimum
        }
        _ if floor.minimum_safe_tag().is_some() || !floor.yanked_tags().is_empty() => {
            SafetyFloorStatus::Safe
        }
        _ => SafetyFloorStatus::Unknown,
    };
    SafetyFloorOutput {
        status,
        minimum_safe_tag: floor.minimum_safe_tag().map(str::to_owned),
        yanked_tags: floor.yanked_tags().to_vec(),
    }
}

fn is_below_minimum_safe(tag: &str, minimum_safe_tag: Option<&str>) -> bool {
    let Some(minimum_safe_tag) = minimum_safe_tag else {
        return false;
    };
    let Ok(tag) = parse_stable_release_tag(tag) else {
        return false;
    };
    let Ok(minimum) = parse_stable_release_tag(minimum_safe_tag) else {
        return false;
    };
    tag < minimum
}

fn apply_command(
    state: UpdateCheckState,
    safety_floor: &SafetyFloorOutput,
    latest: Option<&LatestFreshnessMetadata>,
) -> Option<String> {
    if !matches!(
        state,
        UpdateCheckState::Outdated | UpdateCheckState::Yanked | UpdateCheckState::Unsafe
    ) {
        return None;
    }
    let metadata = latest?;
    if latest_target_is_blocked(metadata.latest_tag(), safety_floor) {
        return None;
    }
    Some(pinned_install_command(metadata.latest_tag()))
}

fn latest_target_is_blocked(tag: &str, safety_floor: &SafetyFloorOutput) -> bool {
    safety_floor.yanked_tags.iter().any(|yanked| yanked == tag)
        || is_below_minimum_safe(tag, safety_floor.minimum_safe_tag.as_deref())
}

fn reinstall_command_for_state(
    state: UpdateCheckState,
    report: &InstallStateReport,
    active_tag: Option<&str>,
) -> Option<String> {
    if !matches!(state, UpdateCheckState::InstallUnhealthy) {
        return None;
    }
    active_tag
        .filter(|tag| release_tag_is_url_safe(tag))
        .map(pinned_install_command)
        .or_else(|| {
            matches!(
                report.state,
                InstallStateKind::MissingActivePointer
                    | InstallStateKind::DanglingActivePointer
                    | InstallStateKind::LocalDevTree
            )
            .then(latest_install_command)
        })
}

fn pinned_install_command(tag: &str) -> String {
    format!(
        "curl -fsSL {} | sudo sh",
        crate::release_urls::release_install_url(tag)
    )
}

fn latest_install_command() -> String {
    format!(
        "curl -fsSL {} | sudo sh",
        crate::release_urls::latest_install_url()
    )
}

fn release_tag_is_url_safe(tag: &str) -> bool {
    !tag.is_empty()
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn proof_cache_status(report: &InstallStateReport) -> UpdateProofCacheStatus {
    if report.state == InstallStateKind::LocalDevTree {
        return UpdateProofCacheStatus::LocalDevInstall;
    }
    if matches!(
        report.state,
        InstallStateKind::MissingActivePointer | InstallStateKind::DanglingActivePointer
    ) {
        return UpdateProofCacheStatus::MissingActiveInstall;
    }
    if report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == InstallStateDiagnosticCode::ProofCacheStale)
    {
        return UpdateProofCacheStatus::StaleManifest;
    }
    if report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == InstallStateDiagnosticCode::ProofCacheInvalid)
    {
        return UpdateProofCacheStatus::InvalidManifest;
    }
    if report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == InstallStateDiagnosticCode::ProofCacheMissing)
    {
        return UpdateProofCacheStatus::MissingManifest;
    }
    if report
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.proof_cache.as_ref())
        .is_some()
    {
        return UpdateProofCacheStatus::Available;
    }
    UpdateProofCacheStatus::Unavailable
}

fn current_unix_seconds() -> UnixSeconds {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or(0);
    UnixSeconds::new(seconds)
}

mod active_kind;
mod latest_status;
mod render;

#[cfg(test)]
mod tests;
