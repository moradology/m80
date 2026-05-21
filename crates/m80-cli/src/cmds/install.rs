use std::path::{Path, PathBuf};

use m80_firecracker::{ConfigError, FcError};
use serde::Serialize;

use crate::args::InstallArgs;
use crate::release::{VersionIdentity, VersionStatus};
use crate::release_asset_index;
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
        Err(err) => return Ok(render_install_error(&err, json_mode)),
    };

    if args.dry_run {
        render_install_plan(&plan, json_mode);
    } else {
        let summary = match layout::install_bundle_layout(&plan) {
            Ok(summary) => summary,
            Err(err) => return Ok(errors::render_error(&err, json_mode)),
        };
        render_layout_summary(&summary, json_mode);
    }
    Ok(0)
}

fn install_plan(
    args: &InstallArgs,
    identity: &VersionIdentity,
) -> Result<InstallPlan, InstallError> {
    let source = selected_source(args)?;
    let source = source_plan(source, identity)?;
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
    let source = source_plan_with_index_resolver(source, identity, resolve_indexed_bundle)?;
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
    source: InstallSource<'_>,
    identity: &VersionIdentity,
) -> Result<SourcePlan, InstallError> {
    source_plan_with_index_resolver(source, identity, |tag, identity| {
        release_asset_index::select_release_bundle_for_install(tag, identity)
    })
}

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
    match source {
        InstallSource::ReleaseTag(tag) => {
            validate_tag("release-tag", tag)?;
            validate_tag_source_matches_binary("--release-tag", tag, identity)?;
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
            layout::preflight_attestation_verifier_for_bundle_url(url)?;
            let release_tag = release_tag_from_bundle_url(url);
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

fn validate_tag(field: &'static str, tag: &str) -> Result<(), FcError> {
    let Some(version) = tag.strip_prefix('v') else {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field,
            reason: "stable release tag must be vMAJOR.MINOR.PATCH".into(),
        }));
    };
    let parts = version.split('.').collect::<Vec<_>>();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field,
            reason: format!(
                "stable release tag must be vMAJOR.MINOR.PATCH with no prerelease suffix: {tag}"
            ),
        }));
    }
    Ok(())
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
        println!("installed bundle layout");
        println!("release_tag={}", summary.release_tag);
        println!("version_dir={}", summary.version_dir);
        println!("files_copied={}", summary.files_copied);
        println!("install_provenance={}", summary.install_provenance);
        println!("host_binaries_manifest={}", summary.host_binaries_manifest);
        println!("profile_path={}", summary.profile_path);
        println!("active_pointer={}", summary.active_pointer);
        println!("active_pointer_flipped={}", summary.active_pointer_flipped);
        println!("profile_written={}", summary.profile_written);
        println!("preflight_gate={}", summary.preflight_gate);
        println!(
            "finalization_order={}",
            summary.finalization_order.join(",")
        );
    }
}

fn render_install_error(err: &InstallError, json_mode: bool) -> i32 {
    match err {
        InstallError::Fc(err) => errors::render_error(err, json_mode),
        InstallError::AssetIndex(err) => render_asset_index_error(err, json_mode),
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

#[derive(Debug)]
enum InstallError {
    Fc(FcError),
    AssetIndex(Box<release_asset_index::AssetIndexFailure>),
}

impl InstallError {
    fn asset_index(value: release_asset_index::AssetIndexFailure) -> Self {
        Self::AssetIndex(Box::new(value))
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
