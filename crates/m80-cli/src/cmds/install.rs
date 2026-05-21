use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use m80_firecracker::{ConfigError, FcError};
use serde::Serialize;

use crate::args::InstallArgs;
use crate::release::{VersionIdentity, VersionStatus};
use crate::release_asset_index;
use crate::release_policy::{
    classify_release_tag, release_transition, ReleaseIdentity, ReleaseTransitionReport,
    ReleaseTransitionState,
};
use crate::{errors, json, request_id};

mod layout;

/// `m80 install` — validate installer inputs and render a plan.
pub(super) fn cmd_install(args: InstallArgs, json_mode: bool) -> anyhow::Result<i32> {
    let identity = VersionIdentity::current();
    cmd_install_with_identity(args, json_mode, &identity)
}

fn cmd_install_with_identity(
    args: InstallArgs,
    json_mode: bool,
    identity: &VersionIdentity,
) -> anyhow::Result<i32> {
    let plan = match install_plan(&args, identity) {
        Ok(plan) => plan,
        Err(err) => {
            let err = with_install_retry_context(err, &args);
            return Ok(render_install_error(&err, json_mode));
        }
    };

    if args.dry_run {
        render_install_plan(&plan, json_mode);
    } else {
        let summary = match layout::install_bundle_layout(&plan) {
            Ok(summary) => summary,
            Err(err) => {
                let err = if let Some(bundle_url) = args.bundle_url.as_deref() {
                    layout::with_bundle_url_retry_context(err, bundle_url, &args.install_root)
                } else {
                    err
                };
                return Ok(errors::render_error(&err, json_mode));
            }
        };
        render_layout_summary(&summary, json_mode);
    }
    Ok(0)
}

fn with_install_retry_context(err: InstallError, args: &InstallArgs) -> InstallError {
    let Some(bundle_url) = args.bundle_url.as_deref() else {
        return err;
    };
    match err {
        InstallError::Fc(err) => InstallError::Fc(layout::with_bundle_url_retry_context(
            err,
            bundle_url,
            &args.install_root,
        )),
        other => other,
    }
}

fn install_plan(
    args: &InstallArgs,
    identity: &VersionIdentity,
) -> Result<InstallPlan, InstallError> {
    let source = selected_source(args)?;
    let source = source_plan(&args.install_root, source, identity)?;
    Ok(install_plan_from_source(args, identity, source))
}

#[cfg(test)]
fn install_plan_with_index_resolver<F>(
    args: &InstallArgs,
    identity: &VersionIdentity,
    resolve_indexed_bundle: F,
) -> Result<InstallPlan, InstallError>
where
    F: Fn(
        &str,
        &VersionIdentity,
    ) -> Result<
        release_asset_index::InstallerBundleSelection,
        release_asset_index::AssetIndexFailure,
    >,
{
    let source = selected_source(args)?;
    let source = source_plan_with_index_resolver_for_install(
        &args.install_root,
        source,
        identity,
        resolve_indexed_bundle,
    )?;
    Ok(install_plan_from_source(args, identity, source))
}

fn install_plan_from_source(
    args: &InstallArgs,
    identity: &VersionIdentity,
    source: SourcePlan,
) -> InstallPlan {
    InstallPlan {
        dry_run: args.dry_run,
        install_root: display_path(&args.install_root),
        active_pointer: display_path(&active_pointer(&args.install_root)),
        source,
        binary_version: identity.binary_version.clone(),
        binary_release_tag: identity.release_tag.clone(),
        version_status: identity.version_status.as_str().to_owned(),
    }
}

fn selected_source(args: &InstallArgs) -> Result<InstallSource<'_>, FcError> {
    match (
        args.release_tag.as_deref(),
        args.bundle_url.as_deref(),
        args.bootstrap_tag.as_deref(),
    ) {
        (Some(tag), None, None) => Ok(InstallSource::ReleaseTag(tag)),
        (None, Some(url), None) => Ok(InstallSource::BundleUrl(url)),
        (None, None, Some(tag)) => Ok(InstallSource::BootstrapTag(tag)),
        _ => Err(FcError::Config(ConfigError::InvalidValue {
            field: "install.source",
            reason: "choose exactly one source: --release-tag <TAG> or --bundle-url <URL>".into(),
        })),
    }
}

fn source_plan(
    install_root: &Path,
    source: InstallSource<'_>,
    identity: &VersionIdentity,
) -> Result<SourcePlan, InstallError> {
    source_plan_with_index_resolver_for_install(install_root, source, identity, |tag, identity| {
        release_asset_index::select_release_bundle_for_install(tag, identity)
    })
}

#[cfg(test)]
fn source_plan_with_index_resolver<F>(
    source: InstallSource<'_>,
    identity: &VersionIdentity,
    resolve_indexed_bundle: F,
) -> Result<SourcePlan, InstallError>
where
    F: Fn(
        &str,
        &VersionIdentity,
    ) -> Result<
        release_asset_index::InstallerBundleSelection,
        release_asset_index::AssetIndexFailure,
    >,
{
    source_plan_with_index_resolver_inner(None, source, identity, resolve_indexed_bundle)
}

fn source_plan_with_index_resolver_for_install<F>(
    install_root: &Path,
    source: InstallSource<'_>,
    identity: &VersionIdentity,
    resolve_indexed_bundle: F,
) -> Result<SourcePlan, InstallError>
where
    F: Fn(
        &str,
        &VersionIdentity,
    ) -> Result<
        release_asset_index::InstallerBundleSelection,
        release_asset_index::AssetIndexFailure,
    >,
{
    source_plan_with_index_resolver_inner(
        Some(install_root),
        source,
        identity,
        resolve_indexed_bundle,
    )
}

fn source_plan_with_index_resolver_inner<F>(
    install_root: Option<&Path>,
    source: InstallSource<'_>,
    identity: &VersionIdentity,
    resolve_indexed_bundle: F,
) -> Result<SourcePlan, InstallError>
where
    F: Fn(
        &str,
        &VersionIdentity,
    ) -> Result<
        release_asset_index::InstallerBundleSelection,
        release_asset_index::AssetIndexFailure,
    >,
{
    match source {
        InstallSource::ReleaseTag(tag) => {
            validate_tag("release-tag", tag)?;
            validate_tag_source_matches_binary("--release-tag", tag, identity)?;
            enforce_release_transition_for_target(install_root, tag)?;
            let bundle =
                resolve_indexed_bundle(tag, identity).map_err(InstallError::asset_index)?;
            Ok(SourcePlan {
                kind: SourceKind::PinnedVersion,
                selector: tag.to_owned(),
                release_tag: Some(tag.to_owned()),
                bundle_url: Some(bundle.bundle_url),
            })
        }
        InstallSource::BootstrapTag(tag) => {
            validate_tag("bootstrap-tag", tag)?;
            validate_tag_source_matches_binary("--bootstrap-tag", tag, identity)?;
            enforce_release_transition_for_target(install_root, tag)?;
            let bundle =
                resolve_indexed_bundle(tag, identity).map_err(InstallError::asset_index)?;
            Ok(SourcePlan {
                kind: SourceKind::BootstrapTag,
                selector: tag.to_owned(),
                release_tag: Some(tag.to_owned()),
                bundle_url: Some(bundle.bundle_url),
            })
        }
        InstallSource::BundleUrl(url) => {
            validate_bundle_url(url)?;
            let release_tag = release_tag_from_bundle_url(url);
            if let Some(tag) = release_tag.as_deref() {
                enforce_release_transition_for_target(install_root, tag)?;
            }
            layout::preflight_attestation_verifier_for_bundle_url(url)?;
            validate_bundle_url_matches_binary(release_tag.as_deref(), identity)?;
            Ok(SourcePlan {
                kind: SourceKind::BundleUrl,
                selector: url.to_owned(),
                release_tag,
                bundle_url: Some(url.to_owned()),
            })
        }
    }
}

fn enforce_release_transition_for_target(
    install_root: Option<&Path>,
    target_tag: &str,
) -> Result<(), InstallError> {
    let Some(install_root) = install_root else {
        return Ok(());
    };
    let Some(active_identity) = active_release_identity(install_root)? else {
        return Ok(());
    };
    let report = release_transition(active_identity, classify_release_tag(target_tag));
    match report.state {
        ReleaseTransitionState::UpgradeAllowed | ReleaseTransitionState::AlreadyCurrent => Ok(()),
        _ => Err(InstallError::release_transition(report)),
    }
}

fn active_release_identity(install_root: &Path) -> Result<Option<ReleaseIdentity>, InstallError> {
    let active = active_pointer(install_root);
    let target = match fs::read_link(&active) {
        Ok(target) => target,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(FcError::PathIo {
                path: active,
                source,
            }
            .into());
        }
    };
    let Some(tag) = target.file_name().and_then(|name| name.to_str()) else {
        return Ok(Some(ReleaseIdentity::Malformed {
            tag: target.display().to_string(),
        }));
    };
    Ok(Some(classify_release_tag(tag)))
}

fn validate_tag(field: &'static str, tag: &str) -> Result<(), FcError> {
    crate::release_policy::parse_stable_release_tag(tag)
        .map(|_| ())
        .map_err(|err| stable_tag_error(field, tag, err))
}

fn stable_tag_error(
    field: &'static str,
    tag: &str,
    err: crate::release_policy::ReleaseTagError,
) -> FcError {
    let reason = match err {
        crate::release_policy::ReleaseTagError::MissingPrefix => {
            "stable release tag must be vMAJOR.MINOR.PATCH".to_owned()
        }
        crate::release_policy::ReleaseTagError::Prerelease
        | crate::release_policy::ReleaseTagError::BuildMetadata
        | crate::release_policy::ReleaseTagError::WrongPartCount
        | crate::release_policy::ReleaseTagError::EmptyPart { .. }
        | crate::release_policy::ReleaseTagError::NonDigitPart { .. }
        | crate::release_policy::ReleaseTagError::NumericOverflow { .. } => {
            format!(
                "stable release tag must be vMAJOR.MINOR.PATCH with no prerelease suffix: {tag}"
            )
        }
    };
    FcError::Config(ConfigError::InvalidValue { field, reason })
}

fn validate_bundle_url(url: &str) -> Result<(), FcError> {
    if url.trim().is_empty() {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "bundle-url",
            reason: "bundle URL must not be empty".into(),
        }));
    }
    Ok(())
}

fn validate_tag_source_matches_binary(
    source_flag: &'static str,
    source_tag: &str,
    identity: &VersionIdentity,
) -> Result<(), InstallError> {
    match identity.version_status {
        VersionStatus::Dev => Err(InstallError::asset_index(
            release_asset_index::AssetIndexFailure::dev_build_refused(
                source_flag,
                source_tag,
                identity,
            ),
        )),
        VersionStatus::Mismatch => Err(InstallError::asset_index(
            release_asset_index::AssetIndexFailure::mismatched_build_refused(source_tag, identity),
        )),
        VersionStatus::Release => {
            let binary_tag = identity.release_tag.as_deref().unwrap_or("<missing>");
            if binary_tag == source_tag {
                Ok(())
            } else {
                Err(InstallError::asset_index(
                    release_asset_index::AssetIndexFailure::tag_mismatch_refused(
                        source_tag, binary_tag, identity,
                    ),
                ))
            }
        }
    }
}

fn validate_bundle_url_matches_binary(
    bundle_tag: Option<&str>,
    identity: &VersionIdentity,
) -> Result<(), FcError> {
    if identity.version_status == VersionStatus::Mismatch {
        return Err(mismatched_build_error(identity));
    }

    let Some(bundle_tag) = bundle_tag else {
        return Ok(());
    };
    match identity.version_status {
        VersionStatus::Dev => Err(FcError::Config(ConfigError::InvalidValue {
            field: "install.binary",
            reason: format!(
                "GitHub release bundle URL selects {bundle_tag}, but this m80 binary is {}; use the matching versioned install.sh or a local --bundle-url fixture",
                identity.binary_version
            ),
        })),
        VersionStatus::Mismatch => Err(mismatched_build_error(identity)),
        VersionStatus::Release => {
            let binary_tag = identity.release_tag.as_deref().unwrap_or("<missing>");
            if binary_tag == bundle_tag {
                Ok(())
            } else {
                Err(tag_mismatch_error(bundle_tag, binary_tag))
            }
        }
    }
}

fn mismatched_build_error(identity: &VersionIdentity) -> FcError {
    FcError::Config(ConfigError::InvalidValue {
        field: "install.binary",
        reason: format!(
            "m80 binary was built with release tag {}, but this package expects {}; use a matching release binary before installing",
            identity.release_tag.as_deref().unwrap_or("<missing>"),
            identity.expected_release_tag
        ),
    })
}

fn tag_mismatch_error(source_tag: &str, binary_tag: &str) -> FcError {
    FcError::Config(ConfigError::InvalidValue {
        field: "install.source",
        reason: format!(
            "bundle/binary tag mismatch: source selects {source_tag}, but this m80 binary is {binary_tag}"
        ),
    })
}

fn release_tag_from_bundle_url(url: &str) -> Option<String> {
    layout::official_release_tag_from_bundle_url(url)
        .ok()
        .flatten()
}

fn active_pointer(install_root: &Path) -> PathBuf {
    install_root.join("active")
}

fn display_path(path: &Path) -> String {
    path.display().to_string()
}

fn render_install_plan(plan: &InstallPlan, json_mode: bool) {
    if json_mode {
        println!("{}", json::to_pretty(plan));
    } else {
        println!("install dry-run");
        println!("source_kind={}", plan.source.kind.as_str());
        println!("source={}", plan.source.selector);
        if let Some(tag) = &plan.source.release_tag {
            println!("release_tag={tag}");
        }
        if let Some(url) = &plan.source.bundle_url {
            println!("bundle_url={url}");
        }
        println!("install_root={}", plan.install_root);
        println!("active_pointer={}", plan.active_pointer);
        println!("binary_version={}", plan.binary_version);
        println!("version_status={}", plan.version_status);
        println!("dry_run=true");
        println!("writes=none");
    }
}

fn render_layout_summary(summary: &layout::LayoutInstallSummary, json_mode: bool) {
    if json_mode {
        println!("{}", json::to_pretty(summary));
    } else {
        for line in layout_summary_lines(summary) {
            println!("{line}");
        }
    }
}

fn layout_summary_lines(summary: &layout::LayoutInstallSummary) -> Vec<String> {
    let mut lines = vec![
        "installed bundle layout".to_owned(),
        format!("release_tag={}", summary.release_tag),
        format!("version_dir={}", summary.version_dir),
        format!("files_copied={}", summary.files_copied),
        format!("install_provenance={}", summary.install_provenance),
        format!("host_binaries_manifest={}", summary.host_binaries_manifest),
        format!("profile_path={}", summary.profile_path),
        format!("active_pointer={}", summary.active_pointer),
        format!("active_pointer_flipped={}", summary.active_pointer_flipped),
        format!("profile_written={}", summary.profile_written),
        format!("preflight_gate={}", summary.preflight_gate),
        format!(
            "finalization_order={}",
            summary.finalization_order.join(",")
        ),
    ];
    if let Some(reinstall) = &summary.reinstall {
        lines.push(format!("reinstall_status={}", reinstall.status));
        lines.push(format!(
            "existing_proof_cache_manifest_digest={}",
            reinstall.existing_manifest_digest
        ));
        lines.push(format!(
            "verified_proof_cache_manifest_digest={}",
            reinstall.verified_manifest_digest
        ));
    }
    if let Some(release_material) = &summary.release_material {
        lines.push(format!(
            "release_material_release_tag={}",
            release_material.release_tag
        ));
        lines.push(format!("bundle_asset={}", release_material.bundle_asset));
        lines.push(format!("bundle_url={}", release_material.bundle_url));
        lines.push(format!("bundle_sha256={}", release_material.bundle_sha256));
        lines.push(format!(
            "install_sh_sha256={}",
            release_material.install_sh_sha256
        ));
        lines.push(format!(
            "public_sha256s_sha256={}",
            release_material.public_sha256s_sha256
        ));
        lines.push(format!(
            "asset_index_sha256={}",
            release_material.asset_index_sha256
        ));
        lines.push(format!(
            "predicate_sha256={}",
            release_material.predicate_sha256
        ));
        lines.push(format!(
            "attestation_signer={}",
            release_material.attestation_signer
        ));
        lines.push(format!(
            "attestation_issuer={}",
            release_material.attestation_issuer
        ));
        lines.push(format!("source_commit={}", release_material.source_commit));
        lines.push(format!(
            "proof_cache_destination={}",
            release_material.proof_cache_destination
        ));
        lines.push(format!(
            "proof_cache_written={}",
            release_material.proof_cache_written
        ));
    }
    lines
}

fn render_install_error(err: &InstallError, json_mode: bool) -> i32 {
    match err {
        InstallError::Fc(err) => errors::render_error(err, json_mode),
        InstallError::AssetIndex(err) => render_asset_index_error(err, json_mode),
        InstallError::ReleaseTransition(report) => {
            render_release_transition_error(report, json_mode)
        }
    }
}

fn render_asset_index_error(err: &release_asset_index::AssetIndexFailure, json_mode: bool) -> i32 {
    let exit_code = errors::EXIT_CONFIG;
    let diagnostic = err.diagnostic();
    if json_mode {
        let payload = asset_index_error_payload(err);
        eprintln!("{}", json::to_pretty(&payload));
    } else {
        if let Some(request_id) = request_id::current() {
            eprintln!("error: [{request_id}] release asset index: {err}");
        } else {
            eprintln!("error: release asset index: {err}");
        }
        eprintln!("asset_index_code={}", diagnostic.code.as_str());
        eprintln!("requested_os={}", diagnostic.requested_os);
        eprintln!("requested_arch={}", diagnostic.requested_arch);
        eprintln!("requested_image_kind={}", diagnostic.requested_image_kind);
        eprintln!("requested_release_tag={}", diagnostic.requested_release_tag);
        eprintln!("requested_m80_version={}", diagnostic.requested_m80_version);
        if let Some(index_url) = &diagnostic.index_url {
            eprintln!("index_url={index_url}");
        }
        if let Some(fetch_url) = &diagnostic.fetch_url {
            eprintln!("fetch_url={fetch_url}");
        }
        if let Some(checksum_verification) = &diagnostic.checksum_verification {
            eprintln!("checksum_verification={checksum_verification}");
        }
        if !diagnostic.available_tuples.is_empty() {
            eprintln!("available_tuples={}", diagnostic.available_tuples.join(","));
        }
        if !diagnostic.available_image_kinds.is_empty() {
            eprintln!(
                "available_image_kinds={}",
                diagnostic.available_image_kinds.join(",")
            );
        }
        if !diagnostic.available_m80_versions.is_empty() {
            eprintln!(
                "available_m80_versions={}",
                diagnostic.available_m80_versions.join(",")
            );
        }
        if let Some(url) = &diagnostic.repair_url {
            eprintln!("repair_url={url}");
        }
        if let Some(command) = &diagnostic.repair_command {
            eprintln!("repair_command={command}");
        }
    }
    exit_code
}

fn asset_index_error_payload(
    err: &release_asset_index::AssetIndexFailure,
) -> AssetIndexErrorEnvelope {
    AssetIndexErrorEnvelope {
        variant: "ReleaseAssetIndex",
        exit_code: errors::EXIT_CONFIG,
        diagnostic: err.diagnostic().clone(),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
struct AssetIndexErrorEnvelope {
    variant: &'static str,
    exit_code: i32,
    #[serde(flatten)]
    diagnostic: release_asset_index::AssetIndexDiagnostic,
}

fn render_release_transition_error(report: &ReleaseTransitionReport, json_mode: bool) -> i32 {
    let exit_code = errors::EXIT_CONFIG;
    let payload = release_transition_error_payload(report);
    if json_mode {
        eprintln!("{}", json::to_pretty(&payload));
    } else {
        if let Some(request_id) = request_id::current() {
            eprintln!(
                "error: [{request_id}] release transition: {}",
                payload.detail
            );
        } else {
            eprintln!("error: release transition: {}", payload.detail);
        }
        eprintln!("release_transition_code={}", payload.code);
        eprintln!(
            "active_tag={}",
            payload.active_tag.as_deref().unwrap_or("<unavailable>")
        );
        eprintln!(
            "requested_tag={}",
            payload.requested_tag.as_deref().unwrap_or("<unavailable>")
        );
        eprintln!("expected_ordering={}", payload.expected_ordering);
        eprintln!("observed_ordering={}", payload.observed_ordering);
        if let Some(command) = &payload.reinstall_active_command {
            eprintln!("reinstall_active_command={command}");
        }
        eprintln!("rollback_command=<unavailable>");
    }
    exit_code
}

fn release_transition_error_payload(
    report: &ReleaseTransitionReport,
) -> ReleaseTransitionErrorEnvelope {
    ReleaseTransitionErrorEnvelope {
        variant: "ReleaseTransition",
        exit_code: errors::EXIT_CONFIG,
        code: report.state.as_str(),
        active_tag: report.active_tag.clone(),
        requested_tag: report.target_tag.clone(),
        expected_ordering: report.expected_ordering,
        observed_ordering: report.observed_ordering,
        detail: report.diagnostic.clone(),
        reinstall_active_command: report
            .active_tag
            .as_deref()
            .filter(|tag| release_tag_is_url_safe(tag))
            .map(pinned_install_command),
        rollback_command: None,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
struct ReleaseTransitionErrorEnvelope {
    variant: &'static str,
    exit_code: i32,
    code: &'static str,
    active_tag: Option<String>,
    requested_tag: Option<String>,
    expected_ordering: &'static str,
    observed_ordering: &'static str,
    detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reinstall_active_command: Option<String>,
    rollback_command: Option<String>,
}

fn pinned_install_command(tag: &str) -> String {
    format!(
        "curl -fsSL https://github.com/moradology/m80/releases/download/{tag}/install.sh | sudo sh"
    )
}

fn release_tag_is_url_safe(tag: &str) -> bool {
    !tag.is_empty()
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

#[derive(Debug)]
enum InstallError {
    Fc(FcError),
    AssetIndex(Box<release_asset_index::AssetIndexFailure>),
    ReleaseTransition(Box<ReleaseTransitionReport>),
}

impl InstallError {
    fn asset_index(value: release_asset_index::AssetIndexFailure) -> Self {
        Self::AssetIndex(Box::new(value))
    }

    fn release_transition(value: ReleaseTransitionReport) -> Self {
        Self::ReleaseTransition(Box::new(value))
    }
}

impl From<FcError> for InstallError {
    fn from(value: FcError) -> Self {
        Self::Fc(value)
    }
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fc(err) => write!(f, "{err}"),
            Self::AssetIndex(err) => write!(f, "{err}"),
            Self::ReleaseTransition(report) => write!(f, "{}", report.diagnostic),
        }
    }
}

impl std::error::Error for InstallError {}

#[derive(Debug, Clone, Copy)]
enum InstallSource<'a> {
    ReleaseTag(&'a str),
    BundleUrl(&'a str),
    BootstrapTag(&'a str),
}

#[derive(Debug, Serialize)]
struct InstallPlan {
    dry_run: bool,
    install_root: String,
    active_pointer: String,
    source: SourcePlan,
    binary_version: String,
    binary_release_tag: Option<String>,
    version_status: String,
}

#[derive(Debug, Serialize)]
struct SourcePlan {
    kind: SourceKind,
    selector: String,
    release_tag: Option<String>,
    bundle_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum SourceKind {
    PinnedVersion,
    BundleUrl,
    BootstrapTag,
}

impl SourceKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::PinnedVersion => "pinned_version",
            Self::BundleUrl => "bundle_url",
            Self::BootstrapTag => "bootstrap_tag",
        }
    }
}

#[cfg(test)]
mod tests;
