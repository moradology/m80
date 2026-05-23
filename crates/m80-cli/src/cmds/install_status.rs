use std::path::{Path, PathBuf};

use m80_firecracker::ConfigSource;
use serde::Serialize;

use crate::args::InstallStatusArgs;
use crate::install_state::{
    resolve_install_state, ActivePointerReport, ActivePointerStatus, InstallAttemptReport,
    InstallAttemptStatus, InstallAttemptType, InstallConfigReport, InstallMetadataReport,
    InstallProfileReport, InstallStateDiagnostic, InstallStateDiagnosticCode, InstallStateKind,
    InstallStatePaths, InstallStateReport, InstallStateRequest, MetadataFileReport,
    MetadataFileStatus, ProofCacheMaterialReport, ProofCacheMetadataReport, ProofCacheReport,
    ProofCacheTrustPolicyReport, ProofCacheVerifierVersionsReport,
};
use crate::json;
use crate::profile::RuntimeProfileReport;

const DEFAULT_INSTALL_ROOT: &str = "/opt/m80";

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
    push_line(&mut text, "mismatch_count", output.mismatches.len());
    for (index, mismatch) in output.mismatches.iter().enumerate() {
        let prefix = format!("mismatch_{index}");
        push_line(
            &mut text,
            &format!("{prefix}_code"),
            mismatch_code_label(mismatch.code),
        );
        push_optional_path(
            &mut text,
            &format!("{prefix}_expected_path"),
            mismatch.expected_path.as_ref(),
        );
        push_optional_path(
            &mut text,
            &format!("{prefix}_observed_path"),
            mismatch.observed_path.as_ref(),
        );
        push_optional(
            &mut text,
            &format!("{prefix}_expected_tag"),
            mismatch.expected_tag.as_deref(),
        );
        push_optional(
            &mut text,
            &format!("{prefix}_observed_tag"),
            mismatch.observed_tag.as_deref(),
        );
        push_optional_string(
            &mut text,
            &format!("{prefix}_expected_source"),
            mismatch.expected_source.map(|source| format!("{source:?}")),
        );
        push_optional_string(
            &mut text,
            &format!("{prefix}_observed_source"),
            mismatch.observed_source.map(|source| format!("{source:?}")),
        );
        push_optional(
            &mut text,
            &format!("{prefix}_expected_value"),
            mismatch.expected_value.as_deref(),
        );
        push_optional(
            &mut text,
            &format!("{prefix}_observed_value"),
            mismatch.observed_value.as_deref(),
        );
        push_line(&mut text, &format!("{prefix}_message"), &mismatch.message);
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
    push_last_attempt(&mut text, &output.last_attempt);
    push_proof_cache(&mut text, &output.proof_cache);
    push_line(&mut text, "diagnostic_count", output.diagnostics.len());
    for (index, diagnostic) in output.diagnostics.iter().enumerate() {
        let prefix = format!("diagnostic_{index}");
        push_line(
            &mut text,
            &format!("{prefix}_code"),
            diagnostic_code_label(diagnostic.code),
        );
        push_optional(&mut text, &format!("{prefix}_field"), diagnostic.field);
        push_optional_path(
            &mut text,
            &format!("{prefix}_path"),
            diagnostic.path.as_ref(),
        );
        push_line(&mut text, &format!("{prefix}_message"), &diagnostic.message);
        push_optional(
            &mut text,
            &format!("{prefix}_repair_command"),
            diagnostic.repair_command.as_deref(),
        );
        push_optional(
            &mut text,
            &format!("{prefix}_rollback_command"),
            diagnostic.rollback_command.as_deref(),
        );
    }
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

fn push_last_attempt(text: &mut String, attempt: &InstallAttemptOutput) {
    push_line(text, "last_attempt_path", attempt.path.display());
    push_line(
        text,
        "last_attempt_status",
        attempt_status_label(attempt.status),
    );
    push_optional(
        text,
        "last_attempt_type",
        attempt.attempt_type.map(attempt_type_label),
    );
    push_optional(
        text,
        "last_attempt_target_tag",
        attempt.target_tag.as_deref(),
    );
    push_optional(
        text,
        "last_attempt_failure_stage",
        attempt.failure_stage.as_deref(),
    );
    push_optional(
        text,
        "last_attempt_repair_command",
        attempt.repair_command.as_deref(),
    );
}

fn push_proof_cache(text: &mut String, proof_cache: &ProofCacheStatusOutput) {
    push_line(
        text,
        "proof_cache_status",
        proof_cache_status_label(proof_cache.status),
    );
    push_line(text, "proof_cache_message", &proof_cache.message);
    push_optional_path(text, "proof_cache_path", proof_cache.cache_dir.as_ref());
    push_optional_path(
        text,
        "proof_cache_manifest_path",
        proof_cache.manifest_path.as_ref(),
    );
    push_optional(
        text,
        "proof_cache_manifest_sha256",
        proof_cache.manifest_sha256.as_deref(),
    );
    push_optional(
        text,
        "proof_cache_manifest_digest",
        proof_cache.manifest_digest.as_deref(),
    );
    push_optional_string(
        text,
        "proof_cache_manifest_modified_unix_seconds",
        proof_cache
            .manifest_modified_unix_seconds
            .map(|seconds| seconds.to_string()),
    );
    push_optional_string(
        text,
        "proof_cache_age_seconds",
        proof_cache
            .cache_age_seconds
            .map(|seconds| seconds.to_string()),
    );
    push_optional(
        text,
        "proof_cache_release_tag",
        proof_cache.release_tag.as_deref(),
    );
    push_optional(
        text,
        "proof_cache_repository",
        proof_cache.repository.as_deref(),
    );
    push_optional(text, "proof_cache_target", proof_cache.target.as_deref());
    if let Some(trust_policy) = &proof_cache.trust_policy {
        push_line(text, "proof_cache_trust_policy_path", &trust_policy.path);
        push_line(
            text,
            "proof_cache_trust_policy_identity",
            &trust_policy.identity,
        );
        push_line(
            text,
            "proof_cache_trust_policy_sha256",
            &trust_policy.sha256,
        );
    } else {
        push_line(text, "proof_cache_trust_policy_path", "<unavailable>");
        push_line(text, "proof_cache_trust_policy_identity", "<unavailable>");
        push_line(text, "proof_cache_trust_policy_sha256", "<unavailable>");
    }
    push_line(
        text,
        "proof_cache_material_count",
        proof_cache.materials.len(),
    );
    for (index, material) in proof_cache.materials.iter().enumerate() {
        let prefix = format!("proof_cache_material_{index}");
        push_line(text, &format!("{prefix}_role"), &material.role);
        push_line(text, &format!("{prefix}_path"), &material.path);
        push_line(text, &format!("{prefix}_sha256"), &material.sha256);
        push_optional_string(
            text,
            &format!("{prefix}_size_bytes"),
            material.size_bytes.map(|size| size.to_string()),
        );
        push_optional(
            text,
            &format!("{prefix}_subject"),
            material.subject.as_deref(),
        );
        push_optional_string(
            text,
            &format!("{prefix}_modified_unix_seconds"),
            material
                .modified_unix_seconds
                .map(|seconds| seconds.to_string()),
        );
    }
    push_line(
        text,
        "proof_cache_diagnostic_count",
        proof_cache.diagnostics.len(),
    );
    for (index, diagnostic) in proof_cache.diagnostics.iter().enumerate() {
        let prefix = format!("proof_cache_diagnostic_{index}");
        push_line(
            text,
            &format!("{prefix}_code"),
            diagnostic_code_label(diagnostic.code),
        );
        push_optional(text, &format!("{prefix}_field"), diagnostic.field);
        push_optional_path(text, &format!("{prefix}_path"), diagnostic.path.as_ref());
        push_line(text, &format!("{prefix}_message"), &diagnostic.message);
    }
    push_optional(
        text,
        "proof_cache_repair_command",
        proof_cache.repair_command.as_deref(),
    );
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
    last_attempt: InstallAttemptOutput,
    proof_cache: ProofCacheStatusOutput,
    diagnostics: Vec<DiagnosticOutput>,
    mismatches: Vec<InstallStatusMismatch>,
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
            last_attempt: InstallAttemptOutput::from_report(&report.last_attempt),
            proof_cache: ProofCacheStatusOutput::from_install_report(report),
            diagnostics: report
                .diagnostics
                .iter()
                .map(|diagnostic| DiagnosticOutput::from_report(diagnostic, report))
                .collect(),
            mismatches: install_status_mismatches(report),
            next_action: next_action(report),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct InstallAttemptOutput {
    path: PathBuf,
    status: InstallAttemptStatus,
    attempt_type: Option<InstallAttemptType>,
    target_tag: Option<String>,
    failure_stage: Option<String>,
    repair_command: Option<String>,
}

impl InstallAttemptOutput {
    fn from_report(report: &InstallAttemptReport) -> Self {
        let attempt = report.attempt.as_ref();
        Self {
            path: report.path.clone(),
            status: report.status,
            attempt_type: attempt.map(|attempt| attempt.attempt_type),
            target_tag: attempt.and_then(|attempt| attempt.target_tag.clone()),
            failure_stage: attempt.and_then(|attempt| attempt.failure_stage.clone()),
            repair_command: attempt.and_then(|attempt| attempt.repair_command.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct DiagnosticOutput {
    code: InstallStateDiagnosticCode,
    field: Option<&'static str>,
    path: Option<PathBuf>,
    message: String,
    repair_command: Option<String>,
    rollback_command: Option<String>,
}

impl DiagnosticOutput {
    fn from_report(diagnostic: &InstallStateDiagnostic, report: &InstallStateReport) -> Self {
        Self {
            code: diagnostic.code,
            field: diagnostic.field,
            path: diagnostic.path.clone(),
            message: diagnostic.message.clone(),
            repair_command: diagnostic_repair_command(diagnostic.code, report),
            rollback_command: diagnostic_rollback_command(diagnostic.code, report),
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

#[derive(Debug, Clone, Serialize)]
pub(super) struct ProofCacheStatusOutput {
    status: ProofCacheStatusKind,
    cache_dir: Option<PathBuf>,
    manifest_path: Option<PathBuf>,
    manifest_sha256: Option<String>,
    manifest_digest: Option<String>,
    manifest_modified_unix_seconds: Option<i64>,
    cache_age_seconds: Option<u64>,
    release_tag: Option<String>,
    repository: Option<String>,
    target: Option<String>,
    materials: Vec<ProofCacheMaterialOutput>,
    trust_policy: Option<ProofCacheTrustPolicyOutput>,
    verifier_versions: Option<ProofCacheVerifierVersionsOutput>,
    diagnostics: Vec<InstallStateDiagnostic>,
    repair_command: Option<String>,
    message: String,
}

impl ProofCacheStatusOutput {
    fn from_install_report(report: &InstallStateReport) -> Self {
        let proof_cache_diagnostics = proof_cache_diagnostics(&report.diagnostics);
        let release_tag = report.active_pointer.release_tag.as_deref().or_else(|| {
            report
                .profile
                .as_ref()
                .and_then(|profile| profile.release_tag.as_deref())
        });
        if let Some(metadata) = &report.metadata {
            return Self::from_metadata(
                &metadata.proof_cache_manifest,
                metadata.proof_cache.as_ref(),
                proof_cache_diagnostics,
                release_tag,
            );
        }
        match report.state {
            InstallStateKind::LocalDevTree | InstallStateKind::ExplicitOverride => {
                Self::non_cache(ProofCacheStatusKind::LocalDevInstall)
            }
            InstallStateKind::MissingActivePointer | InstallStateKind::DanglingActivePointer => {
                Self::non_cache(ProofCacheStatusKind::MissingActiveInstall)
            }
            _ => Self::non_cache(ProofCacheStatusKind::Unavailable),
        }
    }

    pub(super) fn from_runtime_profile(profile: &RuntimeProfileReport) -> Self {
        if profile.body_source == "builtin_env" {
            return Self::non_cache(ProofCacheStatusKind::LocalDevInstall);
        }
        let Some(artifact_dir) = profile.artifact_dir.as_deref() else {
            return Self::non_cache(ProofCacheStatusKind::Unavailable);
        };
        let report =
            crate::install_state::read_proof_cache_metadata_from_artifact_dir(artifact_dir);
        Self::from_proof_cache_metadata(report, profile.release_tag.as_deref())
    }

    fn from_proof_cache_metadata(
        report: ProofCacheMetadataReport,
        release_tag: Option<&str>,
    ) -> Self {
        Self::from_metadata(
            &report.proof_cache_manifest,
            report.proof_cache.as_ref(),
            report.diagnostics,
            release_tag,
        )
    }

    fn from_metadata(
        manifest: &MetadataFileReport,
        proof_cache: Option<&ProofCacheReport>,
        diagnostics: Vec<InstallStateDiagnostic>,
        release_tag: Option<&str>,
    ) -> Self {
        let Some(proof_cache) = proof_cache else {
            let status = proof_cache_status_from_manifest_status(manifest.status);
            return Self {
                status,
                cache_dir: manifest.path.parent().map(Path::to_path_buf),
                manifest_path: Some(manifest.path.clone()),
                manifest_sha256: manifest.sha256.clone(),
                manifest_digest: None,
                manifest_modified_unix_seconds: None,
                cache_age_seconds: None,
                release_tag: None,
                repository: None,
                target: None,
                materials: Vec::new(),
                trust_policy: None,
                verifier_versions: None,
                diagnostics,
                repair_command: proof_cache_repair_command(status, release_tag),
                message: proof_cache_status_message(status).to_owned(),
            };
        };
        Self {
            status: ProofCacheStatusKind::Available,
            cache_dir: Some(proof_cache.cache_dir.clone()),
            manifest_path: Some(proof_cache.manifest_path.clone()),
            manifest_sha256: manifest.sha256.clone(),
            manifest_digest: Some(proof_cache.manifest_digest.clone()),
            manifest_modified_unix_seconds: proof_cache.manifest_modified_unix_seconds,
            cache_age_seconds: proof_cache.cache_age_seconds,
            release_tag: Some(proof_cache.release_tag.clone()),
            repository: Some(proof_cache.repository.clone()),
            target: Some(proof_cache.target.clone()),
            materials: proof_cache
                .materials
                .iter()
                .map(ProofCacheMaterialOutput::from_report)
                .collect(),
            trust_policy: Some(ProofCacheTrustPolicyOutput::from_report(
                &proof_cache.trust_policy,
            )),
            verifier_versions: Some(ProofCacheVerifierVersionsOutput::from_report(
                &proof_cache.verifier_versions,
            )),
            diagnostics,
            repair_command: None,
            message: proof_cache_status_message(ProofCacheStatusKind::Available).to_owned(),
        }
    }

    fn non_cache(status: ProofCacheStatusKind) -> Self {
        Self {
            status,
            cache_dir: None,
            manifest_path: None,
            manifest_sha256: None,
            manifest_digest: None,
            manifest_modified_unix_seconds: None,
            cache_age_seconds: None,
            release_tag: None,
            repository: None,
            target: None,
            materials: Vec::new(),
            trust_policy: None,
            verifier_versions: None,
            diagnostics: Vec::new(),
            repair_command: None,
            message: proof_cache_status_message(status).to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum ProofCacheStatusKind {
    Available,
    MissingActiveInstall,
    LocalDevInstall,
    MissingManifest,
    InvalidManifest,
    StaleManifest,
    Unavailable,
}

#[derive(Debug, Clone, Serialize)]
struct ProofCacheMaterialOutput {
    role: String,
    path: String,
    sha256: String,
    size_bytes: Option<u64>,
    subject: Option<String>,
    modified_unix_seconds: Option<i64>,
}

impl ProofCacheMaterialOutput {
    fn from_report(report: &ProofCacheMaterialReport) -> Self {
        Self {
            role: report.role.clone(),
            path: report.path.clone(),
            sha256: report.sha256.clone(),
            size_bytes: report.size_bytes,
            subject: report.subject.clone(),
            modified_unix_seconds: report.modified_unix_seconds,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct ProofCacheTrustPolicyOutput {
    path: String,
    identity: String,
    sha256: String,
    modified_unix_seconds: Option<i64>,
}

impl ProofCacheTrustPolicyOutput {
    fn from_report(report: &ProofCacheTrustPolicyReport) -> Self {
        Self {
            path: report.path.clone(),
            identity: report.identity.clone(),
            sha256: report.sha256.clone(),
            modified_unix_seconds: report.modified_unix_seconds,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct ProofCacheVerifierVersionsOutput {
    m80_version: String,
    attestation_verifier: String,
    release_integrity_schema_version: u32,
    asset_index_schema_version: u32,
}

impl ProofCacheVerifierVersionsOutput {
    fn from_report(report: &ProofCacheVerifierVersionsReport) -> Self {
        Self {
            m80_version: report.m80_version.clone(),
            attestation_verifier: report.attestation_verifier.clone(),
            release_integrity_schema_version: report.release_integrity_schema_version,
            asset_index_schema_version: report.asset_index_schema_version,
        }
    }
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
struct InstallStatusMismatch {
    code: InstallStatusMismatchCode,
    expected_path: Option<PathBuf>,
    observed_path: Option<PathBuf>,
    expected_tag: Option<String>,
    observed_tag: Option<String>,
    expected_source: Option<ConfigSource>,
    observed_source: Option<ConfigSource>,
    expected_value: Option<String>,
    observed_value: Option<String>,
    message: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum InstallStatusMismatchCode {
    InstallRootOverride,
    ExplicitProfileOverride,
    SelectedProfileUnavailable,
    StaleProfileTarget,
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

fn diagnostic_repair_command(
    code: InstallStateDiagnosticCode,
    report: &InstallStateReport,
) -> Option<String> {
    if report.config.explicit_override {
        return None;
    }
    match code {
        InstallStateDiagnosticCode::MissingActivePointer
        | InstallStateDiagnosticCode::LocalDevProfile => Some(latest_install_command()),
        InstallStateDiagnosticCode::DanglingActivePointer
        | InstallStateDiagnosticCode::ProfileTargetsInactiveVersion
        | InstallStateDiagnosticCode::InstallMetadataMissing
        | InstallStateDiagnosticCode::InstallMetadataInvalid
        | InstallStateDiagnosticCode::InstallMetadataStale
        | InstallStateDiagnosticCode::ProofCacheMissing
        | InstallStateDiagnosticCode::ProofCacheInvalid
        | InstallStateDiagnosticCode::ProofCacheStale => Some(reinstall_command(report)),
        InstallStateDiagnosticCode::ConfigLoadFailed
        | InstallStateDiagnosticCode::DefaultProfileMissing
        | InstallStateDiagnosticCode::ProfileLoadFailed
        | InstallStateDiagnosticCode::ActivePointerUnreadable
        | InstallStateDiagnosticCode::ActivePointerTraversal
        | InstallStateDiagnosticCode::ActivePointerOutsideInstallRoot
        | InstallStateDiagnosticCode::ActivePointerNotVersionDir
        | InstallStateDiagnosticCode::ProfilePathTraversal
        | InstallStateDiagnosticCode::ProfilePathOutsideInstallRoot
        | InstallStateDiagnosticCode::ProfileArtifactDirMalformed
        | InstallStateDiagnosticCode::ExplicitProfileOverride => None,
    }
}

fn diagnostic_rollback_command(
    code: InstallStateDiagnosticCode,
    report: &InstallStateReport,
) -> Option<String> {
    if report.config.explicit_override {
        return None;
    }
    if !matches!(
        code,
        InstallStateDiagnosticCode::MissingActivePointer
            | InstallStateDiagnosticCode::DanglingActivePointer
            | InstallStateDiagnosticCode::ProfileTargetsInactiveVersion
    ) {
        return None;
    }
    let profile = report.profile.as_ref()?;
    let version_dir = profile.version_dir.as_ref()?;
    Some(format!(
        "sudo ln -sfnT -- {} {}",
        shell_quote_path(version_dir),
        shell_quote_path(&report.active_pointer.path)
    ))
}

fn shell_quote_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn release_tag_is_url_safe(tag: &str) -> bool {
    !tag.is_empty()
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn install_status_mismatches(report: &InstallStateReport) -> Vec<InstallStatusMismatch> {
    let mut mismatches = Vec::new();
    if report.install_root != Path::new(DEFAULT_INSTALL_ROOT) {
        mismatches.push(InstallStatusMismatch {
            code: InstallStatusMismatchCode::InstallRootOverride,
            expected_path: Some(PathBuf::from(DEFAULT_INSTALL_ROOT)),
            observed_path: Some(report.install_root.clone()),
            expected_tag: None,
            observed_tag: report.active_pointer.release_tag.clone(),
            expected_source: None,
            observed_source: None,
            expected_value: Some("default install root".to_owned()),
            observed_value: Some("explicit install root".to_owned()),
            message: format!(
                "inspecting explicit install root {}; default status uses {}",
                report.install_root.display(),
                DEFAULT_INSTALL_ROOT
            ),
        });
    }
    if report.config.explicit_override {
        mismatches.push(explicit_profile_override_mismatch(report));
    }
    if report.config.default_profile.is_some() && report.profile.is_none() {
        mismatches.push(InstallStatusMismatch {
            code: InstallStatusMismatchCode::SelectedProfileUnavailable,
            expected_path: None,
            observed_path: None,
            expected_tag: report.active_pointer.release_tag.clone(),
            observed_tag: None,
            expected_source: report.config.default_profile_source,
            observed_source: None,
            expected_value: report.config.default_profile.clone(),
            observed_value: Some("unavailable".to_owned()),
            message: "effective default_profile did not resolve to a runtime profile".to_owned(),
        });
    }
    if !report.config.explicit_override {
        if let (ActivePointerStatus::Live, Some(active_dir), Some(profile)) = (
            report.active_pointer.status,
            report.active_pointer.version_dir.as_ref(),
            report.profile.as_ref(),
        ) {
            if profile.version_dir.as_ref() != Some(active_dir) {
                mismatches.push(InstallStatusMismatch {
                    code: InstallStatusMismatchCode::StaleProfileTarget,
                    expected_path: Some(active_dir.clone()),
                    observed_path: profile.version_dir.clone(),
                    expected_tag: report.active_pointer.release_tag.clone(),
                    observed_tag: profile.release_tag.clone(),
                    expected_source: Some(ConfigSource::SystemFile),
                    observed_source: Some(profile.selection_source),
                    expected_value: Some("active release".to_owned()),
                    observed_value: Some("selected profile release".to_owned()),
                    message:
                        "selected profile points at a different version than the active pointer"
                            .to_owned(),
                });
            }
        }
    }
    mismatches
}

fn explicit_profile_override_mismatch(report: &InstallStateReport) -> InstallStatusMismatch {
    InstallStatusMismatch {
        code: InstallStatusMismatchCode::ExplicitProfileOverride,
        expected_path: report.config.system_path.clone(),
        observed_path: observed_config_source_path(&report.config),
        expected_tag: report.active_pointer.release_tag.clone(),
        observed_tag: report
            .profile
            .as_ref()
            .and_then(|profile| profile.release_tag.clone()),
        expected_source: Some(ConfigSource::SystemFile),
        observed_source: report.config.default_profile_source,
        expected_value: Some("installed default profile".to_owned()),
        observed_value: report.config.default_profile.clone(),
        message: "default_profile came from an explicit override source; installed active state is not authoritative for this invocation".to_owned(),
    }
}

fn observed_config_source_path(config: &InstallConfigReport) -> Option<PathBuf> {
    match config.default_profile_source {
        Some(ConfigSource::SystemFile) => config.system_path.clone(),
        Some(ConfigSource::SystemDropIn) => config.system_drop_in_dir.clone(),
        Some(ConfigSource::UserFile) => config.user_path.clone(),
        Some(ConfigSource::UserDropIn) => config.user_drop_in_dir.clone(),
        Some(ConfigSource::Env | ConfigSource::Flag | ConfigSource::Default) | None => None,
    }
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

fn mismatch_code_label(code: InstallStatusMismatchCode) -> &'static str {
    match code {
        InstallStatusMismatchCode::InstallRootOverride => "install_root_override",
        InstallStatusMismatchCode::ExplicitProfileOverride => "explicit_profile_override",
        InstallStatusMismatchCode::SelectedProfileUnavailable => "selected_profile_unavailable",
        InstallStatusMismatchCode::StaleProfileTarget => "stale_profile_target",
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

fn attempt_status_label(status: InstallAttemptStatus) -> &'static str {
    match status {
        InstallAttemptStatus::Missing => "missing",
        InstallAttemptStatus::Present => "present",
        InstallAttemptStatus::Invalid => "invalid",
    }
}

fn attempt_type_label(attempt_type: InstallAttemptType) -> &'static str {
    match attempt_type {
        InstallAttemptType::SuccessfulUpgrade => "successful_upgrade",
        InstallAttemptType::VerificationFailed => "verification_failed",
        InstallAttemptType::DowngradeRefused => "downgrade_refused",
        InstallAttemptType::RollbackUnsupported => "rollback_unsupported",
    }
}

fn proof_cache_status_label(status: ProofCacheStatusKind) -> &'static str {
    match status {
        ProofCacheStatusKind::Available => "available",
        ProofCacheStatusKind::MissingActiveInstall => "missing_active_install",
        ProofCacheStatusKind::LocalDevInstall => "local_dev_install",
        ProofCacheStatusKind::MissingManifest => "missing_manifest",
        ProofCacheStatusKind::InvalidManifest => "invalid_manifest",
        ProofCacheStatusKind::StaleManifest => "stale_manifest",
        ProofCacheStatusKind::Unavailable => "unavailable",
    }
}

fn proof_cache_status_from_manifest_status(status: MetadataFileStatus) -> ProofCacheStatusKind {
    match status {
        MetadataFileStatus::Missing => ProofCacheStatusKind::MissingManifest,
        MetadataFileStatus::Invalid => ProofCacheStatusKind::InvalidManifest,
        MetadataFileStatus::Stale => ProofCacheStatusKind::StaleManifest,
        MetadataFileStatus::Present => ProofCacheStatusKind::Unavailable,
    }
}

fn proof_cache_status_message(status: ProofCacheStatusKind) -> &'static str {
    match status {
        ProofCacheStatusKind::Available => {
            "cached proof material is available from the installed release tree"
        }
        ProofCacheStatusKind::MissingActiveInstall => {
            "no active installed release is available to provide cached proof material"
        }
        ProofCacheStatusKind::LocalDevInstall => {
            "local development profiles do not have installed release proof cache material"
        }
        ProofCacheStatusKind::MissingManifest => {
            "installed release proof-cache manifest is missing"
        }
        ProofCacheStatusKind::InvalidManifest => {
            "installed release proof-cache manifest is invalid"
        }
        ProofCacheStatusKind::StaleManifest => {
            "installed release proof-cache material no longer matches its manifest"
        }
        ProofCacheStatusKind::Unavailable => "cached proof material is unavailable",
    }
}

fn proof_cache_diagnostics(diagnostics: &[InstallStateDiagnostic]) -> Vec<InstallStateDiagnostic> {
    diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.code,
                InstallStateDiagnosticCode::ProofCacheMissing
                    | InstallStateDiagnosticCode::ProofCacheInvalid
                    | InstallStateDiagnosticCode::ProofCacheStale
            )
        })
        .cloned()
        .collect()
}

fn proof_cache_repair_command(
    status: ProofCacheStatusKind,
    release_tag: Option<&str>,
) -> Option<String> {
    if !matches!(
        status,
        ProofCacheStatusKind::MissingManifest
            | ProofCacheStatusKind::InvalidManifest
            | ProofCacheStatusKind::StaleManifest
    ) {
        return None;
    }
    let tag = release_tag.filter(|tag| release_tag_is_url_safe(tag))?;
    Some(format!(
        "curl -fsSL {} | sudo sh",
        crate::release_urls::release_install_url(tag)
    ))
}

fn diagnostic_code_label(code: InstallStateDiagnosticCode) -> &'static str {
    match code {
        InstallStateDiagnosticCode::ConfigLoadFailed => "config_load_failed",
        InstallStateDiagnosticCode::DefaultProfileMissing => "default_profile_missing",
        InstallStateDiagnosticCode::ProfileLoadFailed => "profile_load_failed",
        InstallStateDiagnosticCode::MissingActivePointer => "missing_active_pointer",
        InstallStateDiagnosticCode::DanglingActivePointer => "dangling_active_pointer",
        InstallStateDiagnosticCode::ActivePointerUnreadable => "active_pointer_unreadable",
        InstallStateDiagnosticCode::ActivePointerTraversal => "active_pointer_traversal",
        InstallStateDiagnosticCode::ActivePointerOutsideInstallRoot => {
            "active_pointer_outside_install_root"
        }
        InstallStateDiagnosticCode::ActivePointerNotVersionDir => "active_pointer_not_version_dir",
        InstallStateDiagnosticCode::ProfilePathTraversal => "profile_path_traversal",
        InstallStateDiagnosticCode::ProfilePathOutsideInstallRoot => {
            "profile_path_outside_install_root"
        }
        InstallStateDiagnosticCode::ProfileArtifactDirMalformed => "profile_artifact_dir_malformed",
        InstallStateDiagnosticCode::ProfileTargetsInactiveVersion => {
            "profile_targets_inactive_version"
        }
        InstallStateDiagnosticCode::InstallMetadataMissing => "install_metadata_missing",
        InstallStateDiagnosticCode::InstallMetadataInvalid => "install_metadata_invalid",
        InstallStateDiagnosticCode::InstallMetadataStale => "install_metadata_stale",
        InstallStateDiagnosticCode::ProofCacheMissing => "proof_cache_missing",
        InstallStateDiagnosticCode::ProofCacheInvalid => "proof_cache_invalid",
        InstallStateDiagnosticCode::ProofCacheStale => "proof_cache_stale",
        InstallStateDiagnosticCode::ExplicitProfileOverride => "explicit_profile_override",
        InstallStateDiagnosticCode::LocalDevProfile => "local_dev_profile",
    }
}

#[cfg(test)]
mod tests;
