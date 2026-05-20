use std::path::{Path, PathBuf};

use m80_firecracker::{ConfigError, FcError};
use serde::Serialize;

use crate::args::InstallArgs;
use crate::release::{VersionIdentity, VersionStatus};
use crate::{errors, json};

mod layout;

/// `m80 install` — validate installer inputs and render a plan.
pub(super) fn cmd_install(args: InstallArgs, json_mode: bool) -> anyhow::Result<i32> {
    let identity = VersionIdentity::current();
    let plan = match install_plan(&args, &identity) {
        Ok(plan) => plan,
        Err(err) => return Ok(errors::render_error(&err, json_mode)),
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

fn install_plan(args: &InstallArgs, identity: &VersionIdentity) -> Result<InstallPlan, FcError> {
    let source = selected_source(args)?;
    let source = source_plan(source, identity)?;
    Ok(InstallPlan {
        dry_run: args.dry_run,
        install_root: display_path(&args.install_root),
        active_pointer: display_path(&active_pointer(&args.install_root)),
        source,
        binary_version: identity.binary_version.clone(),
        binary_release_tag: identity.release_tag.clone(),
        version_status: identity.version_status.as_str().to_owned(),
    })
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
) -> Result<SourcePlan, FcError> {
    match source {
        InstallSource::ReleaseTag(tag) => {
            validate_tag("release-tag", tag)?;
            validate_tag_source_matches_binary("--release-tag", tag, identity)?;
            Ok(SourcePlan {
                kind: SourceKind::PinnedVersion,
                selector: tag.to_owned(),
                release_tag: Some(tag.to_owned()),
                bundle_url: None,
            })
        }
        InstallSource::BootstrapTag(tag) => {
            validate_tag("bootstrap-tag", tag)?;
            validate_tag_source_matches_binary("--bootstrap-tag", tag, identity)?;
            Ok(SourcePlan {
                kind: SourceKind::BootstrapTag,
                selector: tag.to_owned(),
                release_tag: Some(tag.to_owned()),
                bundle_url: None,
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
    if tag.trim().is_empty() {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field,
            reason: "tag must not be empty".into(),
        }));
    }
    if !tag.starts_with('v') {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field,
            reason: "tag must be a concrete GitHub release tag such as v0.1.0".into(),
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
) -> Result<(), FcError> {
    match identity.version_status {
        VersionStatus::Dev => Err(FcError::Config(ConfigError::InvalidValue {
            field: "install.binary",
            reason: format!(
                "{source_flag} install requires a tagged m80 binary; this binary is {} ({})",
                identity.binary_version,
                VersionStatus::Dev.as_str()
            ),
        })),
        VersionStatus::Mismatch => Err(mismatched_build_error(identity)),
        VersionStatus::Release => {
            let binary_tag = identity.release_tag.as_deref().unwrap_or("<missing>");
            if binary_tag == source_tag {
                Ok(())
            } else {
                Err(tag_mismatch_error(source_tag, binary_tag))
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
    let (_, after_marker) = url.split_once("/releases/download/")?;
    let tag = after_marker.split('/').next()?;
    if tag.is_empty() {
        None
    } else {
        Some(tag.to_owned())
    }
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
mod tests {
    use super::*;

    fn args_with_release_tag(tag: &str) -> InstallArgs {
        InstallArgs {
            release_tag: Some(tag.to_owned()),
            bundle_url: None,
            bootstrap_tag: None,
            install_root: PathBuf::from("/tmp/m80-install"),
            dry_run: true,
        }
    }

    fn args_with_bundle_url(url: &str) -> InstallArgs {
        InstallArgs {
            release_tag: None,
            bundle_url: Some(url.to_owned()),
            bootstrap_tag: None,
            install_root: PathBuf::from("/tmp/m80-install"),
            dry_run: true,
        }
    }

    #[test]
    fn release_tag_source_matches_tagged_binary() {
        let identity = VersionIdentity::from_parts("1.2.3", Some("v1.2.3"));
        let plan = install_plan(&args_with_release_tag("v1.2.3"), &identity).unwrap();

        assert_eq!(plan.source.kind, SourceKind::PinnedVersion);
        assert_eq!(plan.source.release_tag.as_deref(), Some("v1.2.3"));
        assert_eq!(plan.version_status, "release");
    }

    #[test]
    fn release_tag_source_rejects_dev_binary() {
        let identity = VersionIdentity::from_parts("1.2.3", None);
        let err = install_plan(&args_with_release_tag("v1.2.3"), &identity).unwrap_err();

        assert!(
            err.to_string().contains("requires a tagged m80 binary"),
            "{err}"
        );
    }

    #[test]
    fn release_tag_source_rejects_binary_tag_mismatch() {
        let identity = VersionIdentity::from_parts("1.2.3", Some("v1.2.3"));
        let err = install_plan(&args_with_release_tag("v9.9.9"), &identity).unwrap_err();

        assert!(
            err.to_string().contains("bundle/binary tag mismatch"),
            "{err}"
        );
    }

    #[test]
    fn bundle_url_rejects_release_binary_tag_mismatch() {
        let identity = VersionIdentity::from_parts("1.2.3", Some("v1.2.3"));
        let err = install_plan(
            &args_with_bundle_url(
                "http://127.0.0.1/releases/download/v9.9.9/m80-linux-x86_64.tar.gz",
            ),
            &identity,
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("bundle/binary tag mismatch"),
            "{err}"
        );
    }

    #[test]
    fn tagged_release_bundle_url_rejects_dev_binary() {
        let identity = VersionIdentity::from_parts("1.2.3", None);
        let err = install_plan(
            &args_with_bundle_url(
                "http://127.0.0.1/releases/download/v1.2.3/m80-linux-x86_64.tar.gz",
            ),
            &identity,
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("GitHub release bundle URL"),
            "{err}"
        );
    }

    #[test]
    fn explicit_bundle_url_allows_dev_binary_for_local_bundle_testing() {
        let identity = VersionIdentity::from_parts("1.2.3", None);
        let plan = install_plan(
            &args_with_bundle_url("file:///tmp/m80-linux-x86_64.tar.gz"),
            &identity,
        )
        .unwrap();

        assert_eq!(plan.source.kind, SourceKind::BundleUrl);
        assert_eq!(
            plan.source.bundle_url.as_deref(),
            Some("file:///tmp/m80-linux-x86_64.tar.gz")
        );
        assert_eq!(plan.version_status, "dev");
    }

    #[test]
    fn release_bundle_tag_is_extracted_from_github_url() {
        let tag = release_tag_from_bundle_url(
            "https://github.com/moradology/m80/releases/download/v1.2.3/m80-linux-x86_64.tar.gz",
        );

        assert_eq!(tag.as_deref(), Some("v1.2.3"));
    }
}
