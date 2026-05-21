use std::path::PathBuf;

use m80_firecracker::ConfigSource;
use serde::Serialize;

use crate::args::InstallStatusArgs;
use crate::install_state::{
    resolve_install_state, ActivePointerReport, ActivePointerStatus, InstallConfigReport,
    InstallMetadataReport, InstallProfileReport, InstallStateDiagnostic, InstallStateKind,
    InstallStatePaths, InstallStateReport, InstallStateRequest, MetadataFileReport,
    MetadataFileStatus,
};
use crate::json;

pub(super) fn cmd_install_status(args: InstallStatusArgs, json_mode: bool) -> anyhow::Result<i32> {
    let report = resolve_install_state(InstallStateRequest {
        paths: InstallStatePaths::host(args.install_root),
        profile_override: args.profile,
    });
    let output = InstallStatusOutput::from_report(&report);
    if json_mode {
        println!("{}", render_json(&output));
    } else {
        print!("{}", render_human(&output));
    }
    Ok(0)
}

fn render_json(output: &InstallStatusOutput) -> String {
    json::to_pretty(output)
}

fn render_human(output: &InstallStatusOutput) -> String {
    let mut text = String::new();
    push_line(&mut text, "status", status_label(output.status));
    push_line(&mut text, "install_root", output.install_root.display());
    push_line(
        &mut text,
        "active_pointer_status",
        active_pointer_status_label(output.active.status),
    );
    push_optional(
        &mut text,
        "active_release_tag",
        output.active.release_tag.as_deref(),
    );
    push_optional_path(
        &mut text,
        "active_install_dir",
        output.active.install_dir.as_ref(),
    );
    push_line(
        &mut text,
        "active_pointer_path",
        output.active.pointer_path.display(),
    );
    push_optional_path(
        &mut text,
        "active_pointer_target",
        output.active.pointer_target.as_ref(),
    );
    push_optional(
        &mut text,
        "selected_config_default_profile",
        output.selected_config.default_profile.as_deref(),
    );
    push_optional_string(
        &mut text,
        "selected_config_default_profile_source",
        output
            .selected_config
            .default_profile_source
            .map(|source| format!("{source:?}")),
    );
    if let Some(profile) = &output.selected_profile {
        push_line(&mut text, "selected_profile", &profile.name);
        push_line(
            &mut text,
            "selected_profile_source",
            format!("{:?}", profile.selection_source),
        );
        push_line(
            &mut text,
            "selected_profile_body_source",
            profile.body_source,
        );
        push_optional_path(
            &mut text,
            "selected_profile_path",
            profile.file_path.as_ref(),
        );
        push_optional_path(
            &mut text,
            "selected_profile_artifact_dir",
            profile.artifact_dir.as_ref(),
        );
        push_optional_path(
            &mut text,
            "selected_profile_install_dir",
            profile.install_dir.as_ref(),
        );
        push_optional(
            &mut text,
            "selected_profile_release_tag",
            profile.release_tag.as_deref(),
        );
    } else {
        push_line(&mut text, "selected_profile", "<unavailable>");
    }
    if let Some(metadata) = &output.metadata {
        push_metadata_file(&mut text, "bundle_metadata", &metadata.bundle_metadata);
        push_metadata_file(
            &mut text,
            "host_binaries_manifest",
            &metadata.host_binaries_manifest,
        );
        push_metadata_file(
            &mut text,
            "install_provenance",
            &metadata.install_provenance,
        );
        push_metadata_file(
            &mut text,
            "proof_cache_manifest",
            &metadata.proof_cache_manifest,
        );
    } else {
        push_unavailable_metadata_file(&mut text, "bundle_metadata");
        push_unavailable_metadata_file(&mut text, "host_binaries_manifest");
        push_unavailable_metadata_file(&mut text, "install_provenance");
        push_unavailable_metadata_file(&mut text, "proof_cache_manifest");
    }
    push_line(&mut text, "diagnostic_count", output.diagnostics.len());
    push_line(&mut text, "next_action", &output.next_action.message);
    if let Some(command) = &output.next_action.command {
        push_line(&mut text, "next_action_command", command);
    }
    text
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

fn push_optional_path(text: &mut String, key: &str, value: Option<&PathBuf>) {
    match value {
        Some(path) => push_line(text, key, path.display()),
        None => push_line(text, key, "<unavailable>"),
    }
}

fn push_metadata_file(text: &mut String, prefix: &str, file: &MetadataFileOutput) {
    push_line(text, &format!("{prefix}_path"), file.path.display());
    push_line(
        text,
        &format!("{prefix}_status"),
        metadata_status_label(file.status),
    );
}

fn push_unavailable_metadata_file(text: &mut String, prefix: &str) {
    push_line(text, &format!("{prefix}_path"), "<unavailable>");
    push_line(text, &format!("{prefix}_status"), "unavailable");
}

#[derive(Debug, Clone, Serialize)]
struct InstallStatusOutput {
    schema_version: u16,
    status: InstallStateKind,
    install_root: PathBuf,
    active: ActiveStatusOutput,
    selected_config: ConfigStatusOutput,
    selected_profile: Option<ProfileStatusOutput>,
    metadata: Option<MetadataStatusOutput>,
    diagnostics: Vec<InstallStateDiagnostic>,
    next_action: NextAction,
}

impl InstallStatusOutput {
    fn from_report(report: &InstallStateReport) -> Self {
        Self {
            schema_version: 1,
            status: report.state,
            install_root: report.install_root.clone(),
            active: ActiveStatusOutput::from_report(&report.active_pointer),
            selected_config: ConfigStatusOutput::from_report(&report.config),
            selected_profile: report
                .profile
                .as_ref()
                .map(ProfileStatusOutput::from_report),
            metadata: report
                .metadata
                .as_ref()
                .map(MetadataStatusOutput::from_report),
            diagnostics: report.diagnostics.clone(),
            next_action: next_action(report),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct ActiveStatusOutput {
    status: ActivePointerStatus,
    release_tag: Option<String>,
    install_dir: Option<PathBuf>,
    pointer_path: PathBuf,
    pointer_target: Option<PathBuf>,
}

impl ActiveStatusOutput {
    fn from_report(report: &ActivePointerReport) -> Self {
        Self {
            status: report.status,
            release_tag: report.release_tag.clone(),
            install_dir: report.version_dir.clone(),
            pointer_path: report.path.clone(),
            pointer_target: report.target.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct ConfigStatusOutput {
    system_path: Option<PathBuf>,
    system_drop_in_dir: Option<PathBuf>,
    user_path: Option<PathBuf>,
    user_drop_in_dir: Option<PathBuf>,
    default_profile: Option<String>,
    default_profile_source: Option<ConfigSource>,
    explicit_override: bool,
}

impl ConfigStatusOutput {
    fn from_report(report: &InstallConfigReport) -> Self {
        Self {
            system_path: report.system_path.clone(),
            system_drop_in_dir: report.system_drop_in_dir.clone(),
            user_path: report.user_path.clone(),
            user_drop_in_dir: report.user_drop_in_dir.clone(),
            default_profile: report.default_profile.clone(),
            default_profile_source: report.default_profile_source,
            explicit_override: report.explicit_override,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct ProfileStatusOutput {
    name: String,
    selection_source: ConfigSource,
    body_source: &'static str,
    file_path: Option<PathBuf>,
    artifact_dir: Option<PathBuf>,
    install_dir: Option<PathBuf>,
    release_tag: Option<String>,
    m80_version: Option<String>,
}

impl ProfileStatusOutput {
    fn from_report(report: &InstallProfileReport) -> Self {
        Self {
            name: report.name.clone(),
            selection_source: report.selection_source,
            body_source: report.body_source,
            file_path: report.file_path.clone(),
            artifact_dir: report.artifact_dir.clone(),
            install_dir: report.version_dir.clone(),
            release_tag: report.release_tag.clone(),
            m80_version: report.m80_version.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct MetadataStatusOutput {
    bundle_metadata: MetadataFileOutput,
    host_binaries_manifest: MetadataFileOutput,
    install_provenance: MetadataFileOutput,
    proof_cache_manifest: MetadataFileOutput,
}

impl MetadataStatusOutput {
    fn from_report(report: &InstallMetadataReport) -> Self {
        Self {
            bundle_metadata: MetadataFileOutput::from_report(&report.bundle_metadata),
            host_binaries_manifest: MetadataFileOutput::from_report(&report.host_binaries_manifest),
            install_provenance: MetadataFileOutput::from_report(&report.install_provenance),
            proof_cache_manifest: MetadataFileOutput::from_report(&report.proof_cache_manifest),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct MetadataFileOutput {
    path: PathBuf,
    status: MetadataFileStatus,
    sha256: Option<String>,
}

impl MetadataFileOutput {
    fn from_report(report: &MetadataFileReport) -> Self {
        Self {
            path: report.path.clone(),
            status: report.status,
            sha256: report.sha256.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct NextAction {
    kind: NextActionKind,
    message: String,
    command: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum NextActionKind {
    Ready,
    InstallRelease,
    ReinstallRelease,
    RemoveOverride,
}

fn next_action(report: &InstallStateReport) -> NextAction {
    match report.state {
        InstallStateKind::HealthyActiveRelease => NextAction {
            kind: NextActionKind::Ready,
            message: "installed release is ready".to_owned(),
            command: Some("m80 run -- echo hello".to_owned()),
        },
        InstallStateKind::ExplicitOverride => NextAction {
            kind: NextActionKind::RemoveOverride,
            message: "remove the profile override to inspect the installed default release"
                .to_owned(),
            command: None,
        },
        InstallStateKind::LocalDevTree => NextAction {
            kind: NextActionKind::InstallRelease,
            message: "install a release so m80 run uses the bundled guest by default".to_owned(),
            command: Some(latest_install_command()),
        },
        InstallStateKind::MissingActivePointer => NextAction {
            kind: NextActionKind::InstallRelease,
            message: "install a release to create the active install pointer".to_owned(),
            command: Some(latest_install_command()),
        },
        InstallStateKind::DanglingActivePointer
        | InstallStateKind::StaleProfileTarget
        | InstallStateKind::MissingInstallMetadata
        | InstallStateKind::StaleInstallMetadata
        | InstallStateKind::TamperedProofCache
        | InstallStateKind::InvalidInstallMetadata => NextAction {
            kind: NextActionKind::ReinstallRelease,
            message: "reinstall the selected release to refresh the installed bundle".to_owned(),
            command: Some(reinstall_command(report)),
        },
    }
}

fn reinstall_command(report: &InstallStateReport) -> String {
    report
        .active_pointer
        .release_tag
        .as_deref()
        .or_else(|| {
            report
                .profile
                .as_ref()
                .and_then(|profile| profile.release_tag.as_deref())
        })
        .filter(|tag| release_tag_is_url_safe(tag))
        .map(|tag| {
            format!(
                "curl -fsSL https://github.com/moradology/m80/releases/download/{tag}/install.sh | sudo sh"
            )
        })
        .unwrap_or_else(latest_install_command)
}

fn latest_install_command() -> String {
    "curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh"
        .to_owned()
}

fn release_tag_is_url_safe(tag: &str) -> bool {
    !tag.is_empty()
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn status_label(status: InstallStateKind) -> &'static str {
    match status {
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

fn active_pointer_status_label(status: ActivePointerStatus) -> &'static str {
    match status {
        ActivePointerStatus::Live => "live",
        ActivePointerStatus::Missing => "missing",
        ActivePointerStatus::Dangling => "dangling",
        ActivePointerStatus::Invalid => "invalid",
    }
}

fn metadata_status_label(status: MetadataFileStatus) -> &'static str {
    match status {
        MetadataFileStatus::Present => "present",
        MetadataFileStatus::Missing => "missing",
        MetadataFileStatus::Invalid => "invalid",
        MetadataFileStatus::Stale => "stale",
    }
}

#[cfg(test)]
mod tests;
