use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use m80_firecracker::FcError;

use crate::args::QuickstartArgs;
use crate::errors;
use crate::json;
use crate::release::VersionIdentity;

mod artifact;
mod probe;
mod process;
pub(in crate::cmds) mod profile_writer;
mod provenance;
mod release_url;
mod temp_tree;

use artifact::{
    artifact_checksum_url, find_forbidden_artifact, forbidden_artifact_reason,
    verify_tarball_checksum, REQUIRED_ARTIFACTS,
};
use probe::{
    run_echo_probe, write_host_binaries_manifest_for_probe, RUN_ECHO_PROBE_COMMAND,
    RUN_ECHO_PROBE_EGRESS_POLICY,
};
use process::{run_output, run_status};
use profile_writer::{write_installed_default_profile, InstalledDefaultProfile};
use provenance::{release_tag_from_artifact_url, relocate_build_receipt, relocate_manifest};
use release_url::validate_artifact_url_matches_binary;
use temp_tree::TempTree;

pub(crate) fn cmd_quickstart(args: QuickstartArgs, json_output: bool) -> anyhow::Result<i32> {
    if json_output && !args.no_run {
        let err = FcError::Config(m80_firecracker::ConfigError::InvalidValue {
            field: "json",
            reason: "m80 quickstart --json requires --no-run so stdout remains machine-readable"
                .to_owned(),
        });
        return Ok(errors::render_error(&err, json_output));
    }
    if !args.no_run && (args.profile_dir.is_some() || args.config_path.is_some()) {
        let err = FcError::Config(m80_firecracker::ConfigError::InvalidValue {
            field: "quickstart install-root override",
            reason: "--profile-dir and --config-path are only supported with --no-run; the runnable probe reads the host /etc/m80 config/profile locations".to_owned(),
        });
        return Ok(errors::render_error(&err, json_output));
    }

    let artifact_dir = args.artifact_dir.unwrap_or_else(default_artifact_dir);
    let run_root = args.run_root.unwrap_or_else(default_run_root);
    let profile_dir = args.profile_dir.unwrap_or_else(default_profile_dir);
    let config_path = args.config_path.unwrap_or_else(default_config_path);

    match run_quickstart(
        &args.artifact_url,
        &artifact_dir,
        &run_root,
        &profile_dir,
        &config_path,
        args.no_run,
        json_output,
    ) {
        Ok(summary) => {
            if json_output {
                println!("{}", summary_json(&summary));
            } else {
                eprintln!(
                    "installed artifacts under {}",
                    summary.artifact_dir.display()
                );
                print_next_steps(&summary);
            }
            Ok(0)
        }
        Err(e) => Ok(errors::render_error(&e, json_output)),
    }
}

struct QuickstartSummary {
    artifact_dir: PathBuf,
    host_binaries_manifest: PathBuf,
    host_binaries_manifest_generated: bool,
    run_root: PathBuf,
    profile_path: PathBuf,
    config_path: PathBuf,
    next_check_command: Option<&'static str>,
    probe_command: &'static str,
    probe_egress_policy: &'static str,
    ran_probe: bool,
}

fn run_quickstart(
    artifact_url: &str,
    artifact_dir: &Path,
    run_root: &Path,
    profile_dir: &Path,
    config_path: &Path,
    no_run: bool,
    json_output: bool,
) -> Result<QuickstartSummary, FcError> {
    require_absolute_path("artifact_dir", artifact_dir)?;
    require_absolute_path("run_root", run_root)?;
    require_absolute_path("profile_dir", profile_dir)?;
    require_absolute_path("config_path", config_path)?;
    let identity = VersionIdentity::current();
    validate_artifact_url_matches_binary(artifact_url, &identity)?;

    let temp = TempTree::new()?;
    let tarball = temp.path().join("artifacts.tar.gz");
    let tarball_checksum = temp.path().join("artifacts.tar.gz.sha256");
    let extract_dir = temp.path().join("artifacts");
    fs::create_dir(&extract_dir).map_err(|e| FcError::PathIo {
        path: extract_dir.clone(),
        source: e,
    })?;

    if !json_output {
        eprintln!("downloading m80 artifacts: {artifact_url}");
    }
    run_status(
        Command::new("curl")
            .arg("-fsSL")
            .arg(artifact_url)
            .arg("-o")
            .arg(&tarball),
        "curl artifact tarball",
    )?;
    let checksum_url = artifact_checksum_url(artifact_url);
    if !json_output {
        eprintln!("downloading m80 artifact checksum: {checksum_url}");
    }
    run_status(
        Command::new("curl")
            .arg("-fsSL")
            .arg(&checksum_url)
            .arg("-o")
            .arg(&tarball_checksum),
        "curl artifact checksum",
    )?;
    verify_tarball_checksum(&tarball, &tarball_checksum, json_output)?;

    run_status(
        Command::new("tar")
            .arg("-xzf")
            .arg(&tarball)
            .arg("-C")
            .arg(&extract_dir),
        "extract artifact tarball",
    )?;

    let sums = extract_dir.join("SHA256SUMS");
    if !sums.is_file() {
        return Err(FcError::ArtifactMissing { path: sums });
    }
    run_output(
        Command::new("sha256sum")
            .arg("-c")
            .arg("SHA256SUMS")
            .current_dir(&extract_dir),
        "verify artifact checksums",
        json_output,
    )?;

    if let Some(path) = find_forbidden_artifact(&extract_dir)? {
        let name = path
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("forbidden artifact");
        return Err(FcError::Config(
            m80_firecracker::ConfigError::InvalidValue {
                field: "artifact_url",
                reason: forbidden_artifact_reason(name),
            },
        ));
    }

    for file in REQUIRED_ARTIFACTS {
        let path = extract_dir.join(file);
        if !path.is_file() {
            return Err(FcError::ArtifactMissing { path });
        }
    }

    if !no_run {
        if !json_output {
            eprintln!("checking host substrate before installing active artifacts");
        }
        m80_preflight::verify_host_substrate(
            m80_preflight::HostFeaturePreflightConfig::from_env()?
        )?;
    }

    fs::create_dir_all(artifact_dir).map_err(|e| FcError::PathIo {
        path: artifact_dir.to_path_buf(),
        source: e,
    })?;
    fs::set_permissions(artifact_dir, fs::Permissions::from_mode(0o755)).map_err(|e| {
        FcError::PathIo {
            path: artifact_dir.to_path_buf(),
            source: e,
        }
    })?;
    fs::create_dir_all(run_root).map_err(|e| FcError::PathIo {
        path: run_root.to_path_buf(),
        source: e,
    })?;

    for file in REQUIRED_ARTIFACTS {
        let src = extract_dir.join(file);
        let dst = artifact_dir.join(file);
        fs::copy(&src, &dst).map_err(|e| FcError::PathIo {
            path: dst.clone(),
            source: e,
        })?;
        fs::set_permissions(&dst, fs::Permissions::from_mode(0o644)).map_err(|e| {
            FcError::PathIo {
                path: dst,
                source: e,
            }
        })?;
    }
    let manifest_transform = relocate_manifest(artifact_dir)?;
    let receipt_transform = relocate_build_receipt(artifact_dir)?;
    provenance::write_install_provenance(
        artifact_url,
        artifact_dir,
        vec![manifest_transform, receipt_transform],
    )?;

    let host_binaries_manifest = artifact_dir.join("host-binaries.manifest.json");
    let include_jailer_harden = if no_run {
        true
    } else {
        write_host_binaries_manifest_for_probe(artifact_dir)?
    };
    let profile_path = write_installed_default_profile(InstalledDefaultProfile {
        artifact_dir,
        run_root,
        profile_dir,
        config_path,
        binary_config: m80_preflight::BinaryDiscoveryConfig::from_env(),
        include_jailer_harden,
        release_tag: release_tag_from_artifact_url(artifact_url),
        m80_version: identity.binary_version,
        host_binaries_manifest: &host_binaries_manifest,
        adopt_existing_config: false,
        adoption_command: "m80 install --adopt-existing-config".to_owned(),
    })?;

    if !no_run {
        run_echo_probe()?;
    }

    Ok(QuickstartSummary {
        artifact_dir: artifact_dir.to_path_buf(),
        host_binaries_manifest,
        host_binaries_manifest_generated: !no_run,
        run_root: run_root.to_path_buf(),
        profile_path,
        config_path: config_path.to_path_buf(),
        next_check_command: no_run.then_some("m80 preflight"),
        probe_command: RUN_ECHO_PROBE_COMMAND,
        probe_egress_policy: RUN_ECHO_PROBE_EGRESS_POLICY,
        ran_probe: !no_run,
    })
}

fn require_absolute_path(field: &'static str, path: &Path) -> Result<(), FcError> {
    if path.is_absolute() {
        return Ok(());
    }
    Err(FcError::Config(
        m80_firecracker::ConfigError::InvalidValue {
            field,
            reason: format!("{field} must be an absolute path, got {}", path.display()),
        },
    ))
}

fn summary_json(summary: &QuickstartSummary) -> String {
    let kernel = summary.artifact_dir.join("vmlinux");
    let rootfs = summary.artifact_dir.join("output.ext4");
    let manifest = summary.artifact_dir.join("output.ext4.manifest.json");
    let guestd = summary.artifact_dir.join("m80-guestd");
    let obj = serde_json::json!({
        "artifact_dir": summary.artifact_dir.display().to_string(),
        "host_binaries_manifest": summary.host_binaries_manifest.display().to_string(),
        "host_binaries_manifest_generated": summary.host_binaries_manifest_generated,
        "kernel_image": kernel.display().to_string(),
        "rootfs_image": rootfs.display().to_string(),
        "manifest": manifest.display().to_string(),
        "guestd": guestd.display().to_string(),
        "run_root": summary.run_root.display().to_string(),
        "profile_path": summary.profile_path.display().to_string(),
        "config_path": summary.config_path.display().to_string(),
        "next_check_command": summary.next_check_command,
        "probe_command": summary.probe_command,
        "probe_egress_policy": summary.probe_egress_policy,
        "ran_probe": summary.ran_probe,
    });
    json::to_pretty(&obj)
}

fn print_next_steps(summary: &QuickstartSummary) {
    let probe_status = if summary.ran_probe {
        "probe"
    } else {
        "probe skipped"
    };
    eprintln!(
        "default profile: {}\nconfig: {}\nhost manifest: {} ({})\n{}: {} ({})",
        summary.profile_path.display(),
        summary.config_path.display(),
        summary.host_binaries_manifest.display(),
        if summary.host_binaries_manifest_generated {
            "generated"
        } else {
            "not generated by --no-run"
        },
        probe_status,
        summary.probe_command,
        summary.probe_egress_policy,
    );
    if let Some(command) = summary.next_check_command {
        eprintln!("\nNext check:\n  {command}");
    }
    eprintln!(
        "\nNext:\n  m80 run --workspace . --cwd /workspace -- ls\n  m80 run --egress none -- echo isolated\n  m80 run -it --workspace . -- sh"
    );
}

fn default_artifact_dir() -> PathBuf {
    env_path("M80_ARTIFACT_DIR", "/opt/m80/artifacts")
}

fn default_run_root() -> PathBuf {
    env_path("M80_RUN_ROOT", "/var/run/m80")
}

fn default_profile_dir() -> PathBuf {
    PathBuf::from("/etc/m80/profiles")
}

fn default_config_path() -> PathBuf {
    PathBuf::from("/etc/m80/config.toml")
}

fn env_path(key: &str, default: &str) -> PathBuf {
    std::env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}
