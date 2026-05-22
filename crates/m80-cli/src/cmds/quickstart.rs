use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use m80_firecracker::FcError;
use m80_image_manifest::{
    BuildReceipt, BuildReceiptArtifact, BuildReceiptArtifactKind, InstallProvenance,
    InstallProvenanceArtifact, InstallProvenanceRewrite, InstallProvenanceTransform,
};

use crate::args::QuickstartArgs;
use crate::errors;
use crate::json;
use crate::release::{VersionIdentity, VersionStatus};

pub(in crate::cmds) mod profile_writer;

use profile_writer::{write_installed_default_profile, InstalledDefaultProfile};

const REQUIRED_ARTIFACTS: &[&str] = &[
    "vmlinux",
    "output.ext4",
    "output.ext4.manifest.json",
    "output.ext4.build-receipt.json",
    "m80-guestd",
];
const FORBIDDEN_ARTIFACTS: &[&str] = &[
    "host-binaries.manifest.json",
    "firecracker",
    "jailer",
    "firecracker-seccomp-filter.bin",
    "firecracker-seccomp-filter.json",
];
const INSTALL_PROVENANCE_FILE: &str = "install-provenance.json";
const FORBIDDEN_HOST_PREREQ_REASON: &str = concat!(
    "m80 v0.x release artifacts must not bundle official Firecracker, official jailer, ",
    "or Firecracker seccomp filter payloads; m80 owns m80 binaries/helpers and guest ",
    "artifacts, while Firecracker/jailer/seccomp are operator-provided host prerequisites",
);
const RUN_ECHO_PROBE_COMMAND: &str = "m80 run -- echo hello";
const RUN_ECHO_PROBE_ARGS: &[&str] = &["run", "--", "echo", "hello"];
const RUN_ECHO_PROBE_EGRESS_POLICY: &str = "default-outbound";
const RUN_ECHO_PROBE_ENV_REMOVALS: &[&str] = &[
    "M80_ARTIFACT_DIR",
    "M80_KERNEL_IMAGE",
    "M80_ROOTFS_IMAGE",
    "M80_KERNEL_KIND",
    "M80_RUN_ROOT",
    "M80_DEFAULT_PROFILE",
    "M80_MAX_CONCURRENT_VMS",
    "M80_JAIL_UID",
    "M80_JAIL_GID",
    "M80_CGROUP_MODE",
    "M80_FIRECRACKER_BIN",
    "M80_FIRECRACKER_VERSION",
    "M80_FIRECRACKER_SECCOMP_FILTER",
    "M80_JAILER_BIN",
    "M80_JAILER_HARDEN_BIN",
    "M80_NET_HELPER_BIN",
    "M80_SKIP_CHECK_VULNERABILITIES",
    "M80_FORCE_PREFLIGHT",
    "M80_PHASE_TRACE",
];

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
    write_install_provenance(
        artifact_url,
        artifact_dir,
        vec![manifest_transform, receipt_transform],
    )?;

    let host_binaries_manifest = artifact_dir.join("host-binaries.manifest.json");
    if !no_run {
        write_host_binaries_manifest_for_probe(artifact_dir)?;
    }
    let profile_path = write_installed_default_profile(InstalledDefaultProfile {
        artifact_dir,
        run_root,
        profile_dir,
        config_path,
        binary_config: m80_preflight::BinaryDiscoveryConfig::from_env(),
        release_tag: release_tag_from_artifact_url(artifact_url),
        m80_version: identity.binary_version,
        host_binaries_manifest: &host_binaries_manifest,
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

fn artifact_checksum_url(artifact_url: &str) -> String {
    format!("{artifact_url}.sha256")
}

fn validate_artifact_url_matches_binary(
    artifact_url: &str,
    identity: &VersionIdentity,
) -> Result<(), FcError> {
    let Some(bundle_tag) = public_release_tag_from_artifact_url(artifact_url) else {
        return Ok(());
    };
    if bundle_tag == "latest" {
        return Err(FcError::Config(
            m80_firecracker::ConfigError::InvalidValue {
                field: "artifact_url",
                reason: latest_artifact_url_reason(),
            },
        ));
    }
    if identity.version_status == VersionStatus::Release
        && identity.release_tag.as_deref() == Some(bundle_tag.as_str())
    {
        return Ok(());
    }
    Err(FcError::Config(
        m80_firecracker::ConfigError::InvalidValue {
            field: "artifact_url",
            reason: quickstart_bundle_mismatch_reason(&bundle_tag, identity),
        },
    ))
}

fn public_release_tag_from_artifact_url(artifact_url: &str) -> Option<String> {
    let latest_prefix = format!(
        "https://github.com{}",
        crate::release_urls::latest_download_path_prefix()
    );
    if artifact_url.strip_prefix(&latest_prefix).is_some() {
        return Some("latest".to_owned());
    }

    let prefix = format!(
        "https://github.com{}",
        crate::release_urls::release_download_path_prefix()
    );
    let tail = artifact_url.strip_prefix(&prefix)?;
    let tag = tail.split('/').next()?;
    if tag.is_empty() {
        None
    } else {
        Some(tag.to_owned())
    }
}

fn latest_artifact_url_reason() -> String {
    format!(
        "GitHub latest artifact URLs are mutable and are not valid m80 quickstart inputs; install the latest release with: curl -fsSL {} | sudo sh",
        crate::release_urls::latest_install_url()
    )
}

fn quickstart_bundle_mismatch_reason(bundle_tag: &str, identity: &VersionIdentity) -> String {
    let install_command = format!(
        "curl -fsSL {} | sudo sh",
        crate::release_urls::release_install_url(bundle_tag)
    );
    match identity.version_status {
        VersionStatus::Dev => format!(
            "GitHub release artifact URL selects {bundle_tag}, but this m80 binary is dev build {}; run this exact pinned release command next: {install_command}",
            identity.binary_version
        ),
        VersionStatus::Mismatch => format!(
            "GitHub release artifact URL selects {bundle_tag}, but this m80 binary was built with mismatched release tag {}; rebuild with matching M80_RELEASE_TAG or install the selected release with: {install_command}",
            identity.binary_version
        ),
        VersionStatus::Release => format!(
            "GitHub release artifact URL selects {bundle_tag}, but this m80 binary is {}; use the matching versioned install.sh with: {install_command}",
            identity.binary_version
        ),
    }
}

fn verify_tarball_checksum(
    tarball: &Path,
    checksum_file: &Path,
    json_output: bool,
) -> Result<(), FcError> {
    let expected = read_expected_sha256(checksum_file)?;
    let output = run_output_capture(
        Command::new("sha256sum").arg(tarball),
        "compute artifact tarball checksum",
    )?;
    if !json_output {
        print_command_output(&output.stderr);
    }
    if !output.status.success() {
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let output_text = if combined.is_empty() {
            String::new()
        } else {
            format!(": {combined}")
        };
        return Err(FcError::CommandFailed {
            command: command_label("compute artifact tarball checksum"),
            status: output.status,
            output: output_text,
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let actual = stdout.split_whitespace().next().ok_or_else(|| {
        FcError::Config(m80_firecracker::ConfigError::InvalidValue {
            field: "artifact_url.sha256",
            reason: "sha256sum produced no digest for artifact tarball".to_owned(),
        })
    })?;
    if actual != expected {
        return Err(FcError::Config(
            m80_firecracker::ConfigError::InvalidValue {
                field: "artifact_url.sha256",
                reason: format!(
                    "artifact tarball sha256 mismatch: expected {expected}, got {actual}"
                ),
            },
        ));
    }
    Ok(())
}

fn read_expected_sha256(checksum_file: &Path) -> Result<String, FcError> {
    let text = fs::read_to_string(checksum_file).map_err(|e| FcError::PathIo {
        path: checksum_file.to_path_buf(),
        source: e,
    })?;
    let expected = text.split_whitespace().next().ok_or_else(|| {
        FcError::Config(m80_firecracker::ConfigError::InvalidValue {
            field: "artifact_url.sha256",
            reason: format!("checksum file {} is empty", checksum_file.display()),
        })
    })?;
    if expected.len() != 64 || !expected.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(FcError::Config(
            m80_firecracker::ConfigError::InvalidValue {
                field: "artifact_url.sha256",
                reason: format!(
                    "checksum file {} must start with a 64-hex sha256 digest",
                    checksum_file.display()
                ),
            },
        ));
    }
    Ok(expected.to_ascii_lowercase())
}

fn relocate_manifest(artifact_dir: &Path) -> Result<InstallProvenanceTransform, FcError> {
    let manifest_path = artifact_dir.join("output.ext4.manifest.json");
    let source_sha256 = sha256_file(&manifest_path)?;
    let mut manifest = m80_image_manifest::Manifest::read(&manifest_path)?;
    manifest.kernel_image = artifact_dir.join("vmlinux");
    manifest.output_rootfs_image = artifact_dir.join("output.ext4");
    manifest.daemon_binary_path = artifact_dir.join("m80-guestd");
    if manifest.source_rootfs_image.is_some() {
        manifest.source_rootfs_image = Some(artifact_dir.join("source.ext4"));
    }
    manifest.write(&manifest_path).map_err(FcError::Manifest)?;
    manifest.verify(artifact_dir).map_err(FcError::Manifest)?;
    let installed_sha256 = sha256_file(&manifest_path)?;
    Ok(install_path_rewrite_transform(
        InstallProvenanceArtifact::GuestManifest,
        "output.ext4.manifest.json",
        manifest_path,
        source_sha256,
        installed_sha256,
    ))
}

fn relocate_build_receipt(artifact_dir: &Path) -> Result<InstallProvenanceTransform, FcError> {
    let manifest_path = artifact_dir.join("output.ext4.manifest.json");
    let receipt_path = artifact_dir.join("output.ext4.build-receipt.json");
    let source_sha256 = sha256_file(&receipt_path)?;
    let manifest = m80_image_manifest::Manifest::read(&manifest_path)?;
    let manifest_sha256 = sha256_file(&manifest_path)?;
    let mut artifacts = vec![
        receipt_artifact(
            BuildReceiptArtifactKind::KernelImage,
            manifest.kernel_image.clone(),
            manifest.kernel_image_sha256.clone(),
        ),
        receipt_artifact(
            BuildReceiptArtifactKind::OutputRootfsImage,
            manifest.output_rootfs_image.clone(),
            manifest.output_rootfs_sha256.clone(),
        ),
        receipt_artifact(
            BuildReceiptArtifactKind::DaemonBinaryPath,
            manifest.daemon_binary_path.clone(),
            manifest.daemon_binary_sha256.clone(),
        ),
    ];
    if let (Some(path), Some(sha256)) = (
        manifest.source_rootfs_image.clone(),
        manifest.source_rootfs_sha256.clone(),
    ) {
        artifacts.push(receipt_artifact(
            BuildReceiptArtifactKind::SourceRootfsImage,
            path,
            sha256,
        ));
    }
    BuildReceipt::new(manifest_path, manifest_sha256, artifacts)
        .write(&receipt_path)
        .map_err(FcError::Manifest)?;
    let installed_sha256 = sha256_file(&receipt_path)?;
    Ok(install_path_rewrite_transform(
        InstallProvenanceArtifact::BuildReceipt,
        "output.ext4.build-receipt.json",
        receipt_path,
        source_sha256,
        installed_sha256,
    ))
}

fn receipt_artifact(
    kind: BuildReceiptArtifactKind,
    path: PathBuf,
    sha256: String,
) -> BuildReceiptArtifact {
    BuildReceiptArtifact { kind, path, sha256 }
}

fn install_path_rewrite_transform(
    artifact: InstallProvenanceArtifact,
    source_name: &str,
    installed_path: PathBuf,
    source_sha256: String,
    installed_sha256: String,
) -> InstallProvenanceTransform {
    InstallProvenanceTransform {
        artifact,
        source_sha256,
        source_path: PathBuf::from(source_name),
        installed_sha256,
        installed_path,
        rewrite: InstallProvenanceRewrite::InstallPathRewrite,
    }
}

fn write_install_provenance(
    artifact_url: &str,
    artifact_dir: &Path,
    transforms: Vec<InstallProvenanceTransform>,
) -> Result<(), FcError> {
    InstallProvenance::new(release_tag_from_artifact_url(artifact_url), transforms)
        .write(&artifact_dir.join(INSTALL_PROVENANCE_FILE))
        .map_err(FcError::Manifest)
}

fn release_tag_from_artifact_url(artifact_url: &str) -> Option<String> {
    let (_, tail) = artifact_url.split_once("/releases/download/")?;
    let tag = tail.split('/').next()?;
    if tag.is_empty() || tag == "latest" {
        None
    } else {
        Some(tag.to_owned())
    }
}

fn sha256_file(path: &Path) -> Result<String, FcError> {
    let output = run_output_capture(Command::new("sha256sum").arg(path), "compute file checksum")?;
    if !output.status.success() {
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let output_text = if combined.is_empty() {
            String::new()
        } else {
            format!(": {combined}")
        };
        return Err(FcError::CommandFailed {
            command: "compute file checksum",
            status: output.status,
            output: output_text,
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let digest = stdout.split_whitespace().next().ok_or_else(|| {
        FcError::Config(m80_firecracker::ConfigError::InvalidValue {
            field: "sha256sum",
            reason: "sha256sum produced no digest".to_owned(),
        })
    })?;
    Ok(digest.to_owned())
}

fn find_forbidden_artifact(root: &Path) -> Result<Option<PathBuf>, FcError> {
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir).map_err(|source| FcError::PathIo {
            path: dir.clone(),
            source,
        })? {
            let entry = entry.map_err(|source| FcError::PathIo {
                path: dir.clone(),
                source,
            })?;
            let path = entry.path();
            let file_name = entry.file_name();
            if FORBIDDEN_ARTIFACTS
                .iter()
                .any(|forbidden| file_name == OsStr::new(forbidden))
            {
                return Ok(Some(path));
            }
            let file_type = entry.file_type().map_err(|source| FcError::PathIo {
                path: path.clone(),
                source,
            })?;
            if file_type.is_dir() {
                dirs.push(path);
            }
        }
    }
    Ok(None)
}

fn forbidden_artifact_reason(name: &str) -> String {
    if name == "host-binaries.manifest.json" {
        format!(
            "artifact tarball must not contain {name}; generate it after final host install paths are known"
        )
    } else {
        format!("artifact tarball must not contain {name}; {FORBIDDEN_HOST_PREREQ_REASON}")
    }
}

fn write_host_binaries_manifest_for_probe(artifact_dir: &Path) -> Result<(), FcError> {
    let current = std::env::current_exe()
        .map_err(|source| crate::errors::host_io("resolve current executable", source))?;
    let mut config = m80_preflight::HostBinariesManifestConfig::from_env();
    config.m80_bin = current;
    let manifest_path = artifact_dir.join("host-binaries.manifest.json");
    m80_preflight::write_host_binaries_manifest(&config, &manifest_path)?;
    Ok(())
}

fn run_echo_probe() -> Result<(), FcError> {
    let current = std::env::current_exe()
        .map_err(|source| crate::errors::host_io("resolve current executable", source))?;
    let mut command = Command::new(current);
    command.args(RUN_ECHO_PROBE_ARGS);
    for key in RUN_ECHO_PROBE_ENV_REMOVALS {
        command.env_remove(key);
    }
    let status = command
        .status()
        .map_err(|source| FcError::CommandSpawnFailed {
            command: RUN_ECHO_PROBE_COMMAND,
            source,
        })?;
    if !status.success() {
        return Err(FcError::CommandFailed {
            command: RUN_ECHO_PROBE_COMMAND,
            status,
            output: String::new(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        latest_artifact_url_reason, public_release_tag_from_artifact_url,
        quickstart_bundle_mismatch_reason, validate_artifact_url_matches_binary,
        RUN_ECHO_PROBE_ARGS, RUN_ECHO_PROBE_COMMAND, RUN_ECHO_PROBE_EGRESS_POLICY,
        RUN_ECHO_PROBE_ENV_REMOVALS,
    };
    use crate::release::VersionIdentity;

    #[test]
    fn echo_probe_command_is_plain_public_target() {
        assert_eq!(RUN_ECHO_PROBE_COMMAND, "m80 run -- echo hello");
        assert_eq!(RUN_ECHO_PROBE_ARGS, ["run", "--", "echo", "hello"]);
        assert!(!RUN_ECHO_PROBE_ARGS.contains(&"--egress"));
        assert_eq!(RUN_ECHO_PROBE_EGRESS_POLICY, "default-outbound");
    }

    #[test]
    fn echo_probe_scrubs_runtime_env_overrides() {
        for key in [
            "M80_ARTIFACT_DIR",
            "M80_KERNEL_IMAGE",
            "M80_ROOTFS_IMAGE",
            "M80_KERNEL_KIND",
            "M80_RUN_ROOT",
            "M80_DEFAULT_PROFILE",
            "M80_MAX_CONCURRENT_VMS",
            "M80_JAIL_UID",
            "M80_JAIL_GID",
            "M80_CGROUP_MODE",
            "M80_FIRECRACKER_BIN",
            "M80_FIRECRACKER_VERSION",
            "M80_FIRECRACKER_SECCOMP_FILTER",
            "M80_JAILER_BIN",
            "M80_JAILER_HARDEN_BIN",
            "M80_NET_HELPER_BIN",
            "M80_SKIP_CHECK_VULNERABILITIES",
            "M80_FORCE_PREFLIGHT",
            "M80_PHASE_TRACE",
        ] {
            assert!(
                RUN_ECHO_PROBE_ENV_REMOVALS.contains(&key),
                "quickstart probe must remove ambient {key}"
            );
        }
    }

    #[test]
    fn public_release_artifact_url_must_match_release_binary() {
        let identity = VersionIdentity::from_parts(
            "1.2.3",
            Some("v1.2.3"),
            Some("0123456789abcdef0123456789abcdef01234567"),
        );

        validate_artifact_url_matches_binary(
            "https://github.com/moradology/m80/releases/download/v1.2.3/m80-linux-x86_64.tar.gz",
            &identity,
        )
        .unwrap();
    }

    #[test]
    fn dev_binary_rejects_public_release_artifact_with_repair_command() {
        let identity = VersionIdentity::from_parts("1.2.3", None, None);
        let err = validate_artifact_url_matches_binary(
            "https://github.com/moradology/m80/releases/download/v1.2.3/m80-linux-x86_64.tar.gz",
            &identity,
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("dev build 1.2.3-dev"), "{err}");
        assert!(
            err.contains(
                "run this exact pinned release command next: curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.3/install.sh | sudo sh"
            ),
            "{err}"
        );
    }

    #[test]
    fn public_latest_artifact_url_is_rejected_as_mutable_legacy_quickstart() {
        let identity = VersionIdentity::from_parts("1.2.3", None, None);
        let err = validate_artifact_url_matches_binary(
            "https://github.com/moradology/m80/releases/latest/download/m80-linux-x86_64.tar.gz",
            &identity,
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("latest artifact URLs are mutable"), "{err}");
        assert!(
            err.contains(
                "curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh"
            ),
            "{err}"
        );
    }

    #[test]
    fn release_binary_rejects_different_public_release_artifact() {
        let identity = VersionIdentity::from_parts(
            "1.2.3",
            Some("v1.2.3"),
            Some("0123456789abcdef0123456789abcdef01234567"),
        );
        let err = validate_artifact_url_matches_binary(
            "https://github.com/moradology/m80/releases/download/v9.9.9/m80-linux-x86_64.tar.gz",
            &identity,
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("selects v9.9.9"), "{err}");
        assert!(err.contains("this m80 binary is v1.2.3"), "{err}");
        assert!(
            err.contains("https://github.com/moradology/m80/releases/download/v9.9.9/install.sh"),
            "{err}"
        );
    }

    #[test]
    fn local_artifact_url_remains_operator_test_override_for_dev_builds() {
        let identity = VersionIdentity::from_parts("1.2.3", None, None);

        validate_artifact_url_matches_binary("file:///tmp/m80-linux-x86_64.tar.gz", &identity)
            .unwrap();
        validate_artifact_url_matches_binary(
            "https://example.invalid/m80-linux-x86_64.tar.gz",
            &identity,
        )
        .unwrap();
    }

    #[test]
    fn public_release_tag_extractor_is_scoped_to_m80_github_release_urls() {
        assert_eq!(
            public_release_tag_from_artifact_url(
                "https://github.com/moradology/m80/releases/download/v1.2.3/m80-linux-x86_64.tar.gz"
            )
            .as_deref(),
            Some("v1.2.3")
        );
        assert_eq!(
            public_release_tag_from_artifact_url(
                "https://example.invalid/releases/download/v1.2.3/m80-linux-x86_64.tar.gz"
            ),
            None
        );
        assert_eq!(
            public_release_tag_from_artifact_url(
                "https://github.com/moradology/m80/releases/latest/download/m80-linux-x86_64.tar.gz"
            )
            .as_deref(),
            Some("latest")
        );
    }

    #[test]
    fn mismatch_reason_points_to_pinned_installer() {
        let identity = VersionIdentity::from_parts(
            "1.2.3",
            Some("v1.2.3"),
            Some("0123456789abcdef0123456789abcdef01234567"),
        );
        let reason = quickstart_bundle_mismatch_reason("v9.9.9", &identity);

        assert!(reason.contains("versioned install.sh"), "{reason}");
        assert!(
            reason.contains("/releases/download/v9.9.9/install.sh"),
            "{reason}"
        );
        assert!(
            latest_artifact_url_reason().contains("/releases/latest/download/install.sh | sudo sh")
        );
    }
}

fn run_status(cmd: &mut Command, label: &str) -> Result<(), FcError> {
    let status = cmd.status().map_err(|source| FcError::CommandSpawnFailed {
        command: command_label(label),
        source,
    })?;
    if !status.success() {
        return Err(FcError::CommandFailed {
            command: command_label(label),
            status,
            output: String::new(),
        });
    }
    Ok(())
}

fn run_output(cmd: &mut Command, label: &str, json_output: bool) -> Result<(), FcError> {
    let command_output = run_output_capture(cmd, label)?;
    if !json_output {
        print_command_output(&command_output.stdout);
        print_command_output(&command_output.stderr);
    }
    if !command_output.status.success() {
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&command_output.stdout),
            String::from_utf8_lossy(&command_output.stderr)
        );
        let output = if combined.is_empty() {
            String::new()
        } else {
            format!(": {combined}")
        };
        return Err(FcError::CommandFailed {
            command: command_label(label),
            status: command_output.status,
            output,
        });
    }
    Ok(())
}

fn run_output_capture(cmd: &mut Command, label: &str) -> Result<std::process::Output, FcError> {
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|source| FcError::CommandSpawnFailed {
            command: command_label(label),
            source,
        })
}

fn command_label(label: &str) -> &'static str {
    match label {
        "curl artifact tarball" => "curl artifact tarball",
        "curl artifact checksum" => "curl artifact checksum",
        "compute artifact tarball checksum" => "compute artifact tarball checksum",
        "compute file checksum" => "compute file checksum",
        "extract artifact tarball" => "extract artifact tarball",
        "verify artifact checksums" => "verify artifact checksums",
        _ => "quickstart helper",
    }
}

fn print_command_output(bytes: &[u8]) {
    if !bytes.is_empty() {
        eprint!("{}", String::from_utf8_lossy(bytes));
    }
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

struct TempTree {
    path: PathBuf,
}

impl TempTree {
    fn new() -> Result<Self, FcError> {
        let path = std::env::temp_dir().join(format!("m80-quickstart-{}", ulid::Ulid::new()));
        fs::create_dir(&path).map_err(|e| FcError::PathIo {
            path: path.clone(),
            source: e,
        })?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
