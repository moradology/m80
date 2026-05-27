use std::path::Path;
use std::process::Command;

use m80_firecracker::{ConfigError, FcError};

use crate::args::InstallSmokeGateArg;

use super::super::InstallPlan;
#[cfg(debug_assertions)]
use super::source;
use super::{shell_single_quote, InstallSelectorPaths};

pub(super) struct SmokeGateResult {
    pub(super) selected_gate: &'static str,
    pub(super) preflight_gate: &'static str,
    pub(super) run_smoke_command: Option<Vec<String>>,
}

pub(super) fn verify_smoke_gate(
    plan: &InstallPlan,
    bundle_url: &str,
    final_dir: &Path,
    selector_paths: &InstallSelectorPaths,
) -> Result<SmokeGateResult, FcError> {
    match plan.smoke_gate {
        InstallSmokeGateArg::PreflightOnly => {
            let preflight_gate = verify_preflight_gate(bundle_url)?;
            Ok(SmokeGateResult {
                selected_gate: "preflight-only",
                preflight_gate,
                run_smoke_command: None,
            })
        }
        InstallSmokeGateArg::RunSmoke => {
            let preflight_gate = verify_live_preflight_for_run_smoke(bundle_url, plan, final_dir)?;
            let command = run_process_smoke(final_dir, selector_paths)?;
            Ok(SmokeGateResult {
                selected_gate: "run-smoke",
                preflight_gate,
                run_smoke_command: Some(command),
            })
        }
    }
}

pub(super) fn host_prerequisite_status(preflight_gate: &str) -> String {
    match preflight_gate {
        "live_preflight" => "passed:live_preflight",
        "hostless_fixture" => "passed:hostless_fixture",
        other => other,
    }
    .to_owned()
}

fn verify_preflight_gate(bundle_url: &str) -> Result<&'static str, FcError> {
    let config = m80_preflight::HostFeaturePreflightConfig::from_env()?;
    let discovery = if use_hostless_fixture_preflight(bundle_url)? {
        m80_preflight::verify_host_substrate_fixture(
            config,
            &m80_preflight::HostSubstrateFixture::supported_root(),
        )?
    } else {
        m80_preflight::verify_host_substrate(config)?
    };
    Ok(match discovery.proof_kind {
        m80_preflight::HostSubstrateProofKind::LivePreflight => "live_preflight",
        m80_preflight::HostSubstrateProofKind::HostlessFixture => "hostless_fixture",
    })
}

fn verify_live_preflight_for_run_smoke(
    bundle_url: &str,
    plan: &InstallPlan,
    final_dir: &Path,
) -> Result<&'static str, FcError> {
    let resolved_tag = resolved_tag_from_version_dir(final_dir);

    if use_hostless_fixture_preflight(bundle_url)? {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "install.smoke_gate",
            reason: format!(
                "selected_gate=run-smoke resolved_tag={resolved_tag} preflight_output=hostless_fixture_refused requires live KVM and cannot use hostless fixture; install_root={}; repair_command=m80 install --bundle-url {} --install-root {} --smoke-gate preflight-only",
                plan.install_root,
                shell_single_quote(bundle_url),
                shell_single_quote(&plan.install_root),
            ),
        }));
    }
    let preflight_gate = verify_preflight_gate(bundle_url)?;
    if preflight_gate != "live_preflight" {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "install.smoke_gate",
            reason: format!(
                "selected_gate=run-smoke resolved_tag={resolved_tag} preflight_output={preflight_gate} requires live_preflight, got {preflight_gate}; install_root={}",
                plan.install_root
            ),
        }));
    }
    Ok(preflight_gate)
}

fn run_process_smoke(
    final_dir: &Path,
    selector_paths: &InstallSelectorPaths,
) -> Result<Vec<String>, FcError> {
    let installed_m80 = final_dir.join("bin/m80");
    let command = process_smoke_command(final_dir);
    let resolved_tag = resolved_tag_from_version_dir(final_dir);

    let output = Command::new(&installed_m80)
        .args(["run", "--", "echo", "hello"])
        .output()
        .map_err(|source| FcError::PathIo {
            path: installed_m80.clone(),
            source,
        })?;
    if !output.status.success() || output.stdout != b"hello\n" {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "install.smoke_gate",
            reason: format!(
                "selected_gate=run-smoke resolved_tag={resolved_tag} preflight_output=live_preflight command={} exit_status={} stdout={} stderr={} active_profile=default config_path={} profile_dir={} repair_command=m80 preflight",
                command.join(" "),
                output
                    .status
                    .code()
                    .map_or_else(|| "signal".to_owned(), |code| code.to_string()),
                String::from_utf8_lossy(&output.stdout).trim_end(),
                String::from_utf8_lossy(&output.stderr).trim_end(),
                selector_paths.config_path.display(),
                selector_paths.profile_dir.display(),
            ),
        }));
    }
    Ok(command)
}

pub(super) fn process_smoke_command(final_dir: &Path) -> Vec<String> {
    vec![
        final_dir.join("bin/m80").display().to_string(),
        "run".to_owned(),
        "--".to_owned(),
        "echo".to_owned(),
        "hello".to_owned(),
    ]
}

fn resolved_tag_from_version_dir(final_dir: &Path) -> &str {
    final_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("<unknown>")
}

fn use_hostless_fixture_preflight(bundle_url: &str) -> Result<bool, FcError> {
    #[cfg(debug_assertions)]
    {
        if std::env::var_os("M80_INSTALL_TEST_HOSTLESS_OFFICIAL").is_some() {
            return Ok(true);
        }
        Ok(std::env::var_os("M80_INSTALL_HOSTLESS_FIXTURE").is_some()
            && source::is_fixture_bundle_url(bundle_url)?)
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = bundle_url;
        Ok(false)
    }
}
