use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use m80_firecracker::{Backend, ConfigError, EffectiveConfig, FcError};
use m80_preflight::{CgroupPreflightMode, Discovery, HostFeaturePreflightConfig, PreflightError};
use serde::Serialize;

use crate::config;
use crate::errors;
use crate::json;
use crate::profile::{self, ProfileFilePaths, RuntimeProfile};

/// Resolve config, run preflight, and build an `Arc<Backend>`.
/// Returns `EffectiveConfig` alongside for callers that need source labels.
pub(crate) fn build_backend(
    flag_overrides: &HashMap<&str, String>,
) -> Result<(Arc<Backend>, EffectiveConfig), FcError> {
    let effective = config::load_effective(flag_overrides)?;
    let runtime_profile = profile::resolve_from_effective(&effective, ProfileFilePaths::host())?;
    backend_from_effective(effective, &runtime_profile)
}

pub(crate) fn build_run_backend(
    profile: Option<String>,
) -> Result<(Arc<Backend>, EffectiveConfig), FcError> {
    let mut flag_overrides = HashMap::new();
    if let Some(profile) = profile {
        flag_overrides.insert("default_profile", profile);
    }
    let effective = config::load_effective(&flag_overrides)?;
    let runtime_profile = profile::resolve_from_effective(&effective, ProfileFilePaths::host())?;
    backend_from_effective(effective, &runtime_profile)
}

fn backend_from_effective(
    effective: EffectiveConfig,
    runtime_profile: &RuntimeProfile,
) -> Result<(Arc<Backend>, EffectiveConfig), FcError> {
    let discovery = m80_preflight::run_with_configs(
        binary_config_for_runtime_profile(runtime_profile),
        artifact_config_for_runtime_profile(&effective, runtime_profile),
        host_feature_config_from_effective(&effective)?,
    )?;
    let backend_config = m80_firecracker::backend_config_from_effective(&effective, discovery)?;
    let backend = Backend::new_with_effective_config(backend_config, effective.clone())?;
    Ok((Arc::new(backend), effective))
}

pub(super) fn binary_config_for_runtime_profile(
    runtime_profile: &RuntimeProfile,
) -> m80_preflight::BinaryDiscoveryConfig {
    let mut config = m80_preflight::BinaryDiscoveryConfig::from_env();
    if let Some(path) = &runtime_profile.firecracker_bin {
        config.firecracker_bin = path.clone();
    }
    if let Some(path) = &runtime_profile.firecracker_seccomp_filter {
        config.firecracker_seccomp_filter = path.clone();
    }
    if let Some(path) = &runtime_profile.jailer_bin {
        config.jailer_bin = path.clone();
    }
    if let Some(path) = &runtime_profile.jailer_harden_bin {
        config.jailer_harden_bin = path.clone();
    }
    if let Some(path) = &runtime_profile.net_helper_bin {
        config.net_helper_bin = path.clone();
    }
    config
}

pub(super) fn artifact_config_for_runtime_profile(
    effective: &EffectiveConfig,
    runtime_profile: &RuntimeProfile,
) -> m80_preflight::ArtifactPreflightConfig {
    let mut config = m80_preflight::ArtifactPreflightConfig::from_env();
    if let Some(path) = &runtime_profile.artifact_dir {
        config.artifact_dir = path.clone();
    }
    if let Some(path) = &runtime_profile.kernel_image {
        config.kernel_image = Some(path.clone());
    }
    if let Some(path) = &runtime_profile.rootfs_image {
        config.rootfs_image = Some(path.clone());
    }
    if let Some(kind) = &runtime_profile.kernel_kind {
        config.kernel_kind = Some(kind.clone());
    }
    if let Some(run_root) = effective_run_root(effective) {
        config.run_root = run_root;
    }
    config
}

fn effective_run_root(effective: &EffectiveConfig) -> Option<PathBuf> {
    effective
        .fields
        .iter()
        .find(|f| f.name == "run_root")
        .map(|f| PathBuf::from(&f.value))
}

/// `m80 preflight` — run host capability checks and render a table.
pub(crate) fn cmd_preflight(json: bool) -> anyhow::Result<i32> {
    let effective = match config::load_effective(&HashMap::new()) {
        Ok(effective) => effective,
        Err(err) => return Ok(errors::render_error(&err, json)),
    };
    let runtime_profile =
        match profile::resolve_from_effective(&effective, ProfileFilePaths::host()) {
            Ok(runtime_profile) => runtime_profile,
            Err(err) => return Ok(errors::render_error(&err, json)),
        };
    let result = preflight_with_runtime_profile(effective, &runtime_profile);
    Ok(render_preflight_result(&runtime_profile, result, json))
}

pub(super) fn preflight_with_effective_config(
    effective: EffectiveConfig,
) -> Result<m80_preflight::Discovery, FcError> {
    let runtime_profile = profile::resolve_from_effective(&effective, ProfileFilePaths::host())?;
    preflight_with_runtime_profile(effective, &runtime_profile)
}

fn preflight_with_runtime_profile(
    effective: EffectiveConfig,
    runtime_profile: &RuntimeProfile,
) -> Result<m80_preflight::Discovery, FcError> {
    m80_preflight::run_with_configs(
        binary_config_for_runtime_profile(runtime_profile),
        artifact_config_for_runtime_profile(&effective, runtime_profile),
        host_feature_config_from_effective(&effective).map_err(FcError::Preflight)?,
    )
    .map_err(FcError::Preflight)
}

pub(super) fn host_feature_config_from_effective(
    effective: &EffectiveConfig,
) -> Result<HostFeaturePreflightConfig, PreflightError> {
    let cgroup_mode = effective
        .fields
        .iter()
        .find(|f| f.name == "cgroup_mode")
        .map(|f| f.value.as_str())
        .unwrap_or("unified-v2");
    let jail_uid = effective_jail_id(effective, "jail_uid", 3000)?;
    let jail_gid = effective_jail_id(effective, "jail_gid", 3000)?;
    let expected_concurrent_vms = effective_expected_concurrent_vms(effective)?;
    Ok(HostFeaturePreflightConfig {
        cgroup_mode: match cgroup_mode {
            "disabled" => CgroupPreflightMode::Disabled,
            "unified-v2" => CgroupPreflightMode::UnifiedV2,
            other => {
                return Err(PreflightError::InvalidCgroupMode {
                    actual: other.to_owned(),
                });
            }
        },
        jail_uid,
        jail_gid,
        expected_concurrent_vms,
    })
}

fn effective_expected_concurrent_vms(effective: &EffectiveConfig) -> Result<u32, PreflightError> {
    let Some(value) = effective
        .fields
        .iter()
        .find(|candidate| candidate.name == "max_concurrent_vms")
        .map(|candidate| candidate.value.as_str())
    else {
        return Ok(8);
    };

    match value.parse::<u32>() {
        Ok(0) | Err(_) => Err(PreflightError::InvalidExpectedConcurrentVms {
            actual: value.to_owned(),
        }),
        Ok(parsed) => Ok(parsed),
    }
}

fn effective_jail_id(
    effective: &EffectiveConfig,
    field: &'static str,
    default: u32,
) -> Result<u32, PreflightError> {
    let Some(value) = effective
        .fields
        .iter()
        .find(|candidate| candidate.name == field)
        .map(|candidate| candidate.value.as_str())
    else {
        return Ok(default);
    };

    value
        .parse()
        .map_err(|_| PreflightError::InvalidJailIdentity {
            field,
            value: value.to_owned(),
        })
}

#[derive(Serialize)]
pub(super) struct PreflightReport {
    pub(super) schema_version: u32,
    pub(super) runtime_profile: profile::RuntimeProfileReport,
    pub(super) host_prerequisites: m80_preflight::HostPrerequisiteResult,
}

#[derive(Serialize)]
pub(super) struct PreflightErrorReport {
    #[serde(flatten)]
    pub(super) error: errors::ErrorEnvelope,
    pub(super) runtime_profile: profile::RuntimeProfileReport,
}

pub(super) fn render_preflight_result(
    runtime_profile: &RuntimeProfile,
    result: Result<Discovery, FcError>,
    json: bool,
) -> i32 {
    let runtime_profile = profile::runtime_profile_report(runtime_profile);
    match result {
        Ok(discovery) => {
            if json {
                let proof = match m80_preflight::HostPrerequisiteResult::from_discovery(&discovery)
                {
                    Ok(proof) => proof,
                    Err(err) => {
                        let fc_err = FcError::Config(ConfigError::InvalidValue {
                            field: "preflight.host_prerequisite_result",
                            reason: err.to_string(),
                        });
                        return errors::render_error(&fc_err, json);
                    }
                };
                let report = PreflightReport {
                    schema_version: 1,
                    runtime_profile,
                    host_prerequisites: proof,
                };
                println!("{}", json::to_pretty(&report));
            } else {
                print!("{}", render_preflight_profile(&runtime_profile));
                println!("{}", discovery.render_table());
            }
            0
        }
        Err(e) => render_preflight_error(&runtime_profile, &e, json),
    }
}

fn render_preflight_error(
    runtime_profile: &profile::RuntimeProfileReport,
    err: &FcError,
    json_mode: bool,
) -> i32 {
    if json_mode {
        let report = PreflightErrorReport {
            error: errors::envelope(err),
            runtime_profile: runtime_profile.clone(),
        };
        eprintln!("{}", json::to_pretty(&report));
        errors::exit_code_for(err)
    } else {
        eprint!("{}", render_preflight_profile(runtime_profile));
        errors::render_error(err, false)
    }
}

fn render_preflight_profile(profile: &profile::RuntimeProfileReport) -> String {
    let mut out = String::new();
    writeln!(out, "profile: {}", profile.name).unwrap();
    writeln!(out, "  selection_source: {}", profile.selection_source).unwrap();
    writeln!(out, "  body_source: {}", profile.body_source).unwrap();
    if let Some(file_path) = &profile.file_path {
        writeln!(out, "  file_path: {}", file_path.display()).unwrap();
    }
    if let Some(artifact_dir) = &profile.artifact_dir {
        writeln!(out, "  artifact_dir: {}", artifact_dir.display()).unwrap();
    }
    if let Some(kernel_image) = &profile.kernel_image {
        writeln!(out, "  kernel_image: {}", kernel_image.display()).unwrap();
    }
    if let Some(rootfs_image) = &profile.rootfs_image {
        writeln!(out, "  rootfs_image: {}", rootfs_image.display()).unwrap();
    }
    if let Some(guestd) = &profile.guestd {
        writeln!(out, "  guestd: {}", guestd.display()).unwrap();
    }
    if let Some(host_binaries_manifest) = &profile.host_binaries_manifest {
        writeln!(
            out,
            "  host_binaries_manifest: {}",
            host_binaries_manifest.display()
        )
        .unwrap();
    }
    if let Some(active_pointer) = &profile.active_pointer {
        writeln!(
            out,
            "  active_pointer: {} status={}",
            active_pointer.display(),
            profile.active_pointer_status.unwrap_or("unknown")
        )
        .unwrap();
    }
    if let Some(active_target) = &profile.active_pointer_target {
        writeln!(out, "  active_pointer_target: {}", active_target.display()).unwrap();
    }
    if let Some(release_tag) = &profile.release_tag {
        writeln!(out, "  release_tag: {release_tag}").unwrap();
    }
    if let Some(m80_version) = &profile.m80_version {
        writeln!(out, "  m80_version: {m80_version}").unwrap();
    }
    if !profile.missing_paths.is_empty() {
        writeln!(out, "  missing_paths:").unwrap();
        for missing in &profile.missing_paths {
            writeln!(
                out,
                "    {}: {} ({})",
                missing.field,
                missing.path.display(),
                missing.reason
            )
            .unwrap();
        }
    }
    out
}
