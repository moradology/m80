use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use m80_firecracker::{ConfigError, FcError};
use m80_image_manifest::{
    HostBinariesManifest, HostBinaryEntry, HostBinaryName, HostLaunchMaterialEntry,
    HostLaunchMaterialName, Manifest,
};

use super::super::InstallPlan;
use super::reinstall_flat::verify_flat_projection;
use super::{bundle, metadata};

const INSTALLED_RELEASE_FILES: &[&str] = &[
    "bin/m80",
    "bin/m80-jailer-harden",
    "bin/m80-net-helper",
    "artifacts/vmlinux",
    "artifacts/output.ext4",
    "artifacts/output.ext4.manifest.json",
    "artifacts/output.ext4.build-receipt.json",
    "artifacts/m80-guestd",
    "artifacts/install-provenance.json",
    "install.sh",
    "bundle.json",
    "SHA256SUMS",
];

const INSTALLED_CONFIG_KEYS: &[&str] = &[
    "default_profile",
    "max_concurrent_vms",
    "run_root",
    "jail_uid",
    "jail_gid",
    "cgroup_mode",
];

const INSTALLED_PROFILE_KEYS: &[&str] = &[
    "artifact_dir",
    "kernel_image",
    "rootfs_image",
    "kernel_kind",
    "guestd",
    "guest_manifest",
    "build_receipt",
    "install_provenance",
    "host_binaries_manifest",
    "firecracker_bin",
    "firecracker_seccomp_filter",
    "jailer_bin",
    "jailer_harden_bin",
    "net_helper_bin",
    "run_root",
    "release_tag",
    "m80_version",
    "description",
];

pub(super) struct SameVersionReinstallVerification {
    pub(super) repair_command: String,
}

pub(super) fn verify_same_version_reinstall(
    plan: &InstallPlan,
    bundle_url: &str,
    install_root: &Path,
    final_dir: &Path,
    expected_dir: &Path,
    release_tag: &str,
) -> Result<SameVersionReinstallVerification, FcError> {
    let repair_command = repair_command(bundle_url, install_root, final_dir);
    require_active_pointer_targets(Path::new(&plan.active_pointer), final_dir, &repair_command)?;
    compare_expected_release_files(final_dir, expected_dir, &repair_command)?;
    verify_installed_bundle_metadata(final_dir, &repair_command)?;
    verify_default_config(install_root, &repair_command)?;
    let profile = verify_default_profile(plan, install_root, release_tag, &repair_command)?;
    verify_flat_projection(install_root, final_dir, release_tag, &repair_command)?;
    verify_host_binaries_manifest(install_root, final_dir, &profile, &repair_command)?;
    Ok(SameVersionReinstallVerification { repair_command })
}

fn require_active_pointer_targets(
    active_pointer: &Path,
    final_dir: &Path,
    repair_command: &str,
) -> Result<(), FcError> {
    let target = fs::read_link(active_pointer).map_err(|source| {
        reinstall_error_with_field(
            "install.active_pointer",
            format!(
                "same-version reinstall found existing version directory but active pointer is unreadable: active_pointer={} source={source}",
                active_pointer.display()
            ),
            repair_command,
        )
    })?;
    if target == final_dir {
        Ok(())
    } else {
        Err(reinstall_error_with_field(
            "install.active_pointer",
            format!(
                "same-version reinstall found existing version directory but active pointer targets a different path: active_pointer={} observed_target={} expected_target={}",
                active_pointer.display(),
                target.display(),
                final_dir.display()
            ),
            repair_command,
        ))
    }
}

fn compare_expected_release_files(
    final_dir: &Path,
    expected_dir: &Path,
    repair_command: &str,
) -> Result<(), FcError> {
    for relative in INSTALLED_RELEASE_FILES {
        let installed = final_dir.join(relative);
        let expected = expected_dir.join(relative);
        let installed_digest = file_fingerprint(&installed, repair_command)?;
        let expected_digest = file_fingerprint(&expected, repair_command)?;
        if installed_digest != expected_digest {
            return Err(reinstall_error(
                format!(
                    "same-version reinstall found installed byte mismatch: path={} expected_sha256={} observed_sha256={} expected_size_bytes={} observed_size_bytes={} expected_mode={:03o} observed_mode={:03o}",
                    installed.display(),
                    expected_digest.sha256,
                    installed_digest.sha256,
                    expected_digest.size_bytes,
                    installed_digest.size_bytes,
                    expected_digest.mode,
                    installed_digest.mode
                ),
                repair_command,
            ));
        }
    }
    Ok(())
}

fn verify_installed_bundle_metadata(final_dir: &Path, repair_command: &str) -> Result<(), FcError> {
    let metadata =
        metadata::read_bundle_metadata(&final_dir.join("bundle.json")).map_err(|err| {
            reinstall_error(
                format!("installed bundle metadata is unreadable: {err}"),
                repair_command,
            )
        })?;
    metadata::verify_bundle_metadata(&metadata).map_err(|err| {
        reinstall_error(
            format!("installed bundle metadata is invalid: {err}"),
            repair_command,
        )
    })?;
    metadata::verify_metadata_hashes(final_dir, &metadata).map_err(|err| {
        reinstall_error(
            format!("installed bundle metadata is stale: {err}"),
            repair_command,
        )
    })?;
    metadata::verify_sha256s_file(final_dir).map_err(|err| {
        reinstall_error(
            format!("installed SHA256SUMS is stale: {err}"),
            repair_command,
        )
    })
}

fn verify_default_config(install_root: &Path, repair_command: &str) -> Result<(), FcError> {
    let selector_paths = super::install_selector_paths(install_root);
    let path = selector_paths.config_path;
    let table = read_toml_table(&path, "installed config", repair_command)?;
    reject_unknown_keys(
        &table,
        INSTALLED_CONFIG_KEYS,
        "installed config",
        repair_command,
    )?;
    require_toml_string(
        &table,
        "default_profile",
        "default",
        "installed config",
        repair_command,
    )?;
    require_toml_string(
        &table,
        "run_root",
        &install_root.join("run").display().to_string(),
        "installed config",
        repair_command,
    )
}

fn verify_default_profile(
    plan: &InstallPlan,
    install_root: &Path,
    release_tag: &str,
    repair_command: &str,
) -> Result<InstalledProfilePaths, FcError> {
    let selector_paths = super::install_selector_paths(install_root);
    let path = selector_paths.default_profile_path();
    let table = read_toml_table(&path, "installed default profile", repair_command)?;
    reject_unknown_keys(
        &table,
        INSTALLED_PROFILE_KEYS,
        "installed default profile",
        repair_command,
    )?;
    let artifacts = install_root.join("artifacts");
    let bin = install_root.join("bin");
    for (field, expected) in [
        ("artifact_dir", artifacts.clone()),
        ("kernel_image", artifacts.join("vmlinux")),
        ("rootfs_image", artifacts.join("output.ext4")),
        ("guestd", artifacts.join("m80-guestd")),
        (
            "guest_manifest",
            artifacts.join("output.ext4.manifest.json"),
        ),
        (
            "build_receipt",
            artifacts.join("output.ext4.build-receipt.json"),
        ),
        (
            "install_provenance",
            artifacts.join("install-provenance.json"),
        ),
        (
            "host_binaries_manifest",
            artifacts.join("host-binaries.manifest.json"),
        ),
        ("jailer_harden_bin", bin.join("m80-jailer-harden")),
        ("net_helper_bin", bin.join("m80-net-helper")),
        ("run_root", install_root.join("run")),
    ] {
        require_toml_string(
            &table,
            field,
            &expected.display().to_string(),
            "installed default profile",
            repair_command,
        )?;
    }
    require_toml_string(
        &table,
        "release_tag",
        release_tag,
        "installed default profile",
        repair_command,
    )?;
    require_toml_string(
        &table,
        "m80_version",
        &plan.binary_version,
        "installed default profile",
        repair_command,
    )?;
    let manifest = Manifest::read(&artifacts.join("output.ext4.manifest.json")).map_err(|err| {
        reinstall_error(
            format!("installed guest manifest is unreadable: {err}"),
            repair_command,
        )
    })?;
    let expected_kernel_kind = match manifest.kernel_kind {
        m80_image_manifest::KernelKind::Stock => "stock",
        m80_image_manifest::KernelKind::Stripped => "stripped",
    };
    require_toml_string(
        &table,
        "kernel_kind",
        expected_kernel_kind,
        "installed default profile",
        repair_command,
    )?;
    for field in [
        "firecracker_bin",
        "firecracker_seccomp_filter",
        "jailer_bin",
    ] {
        let value = toml_string(&table, field, "installed default profile", repair_command)?;
        if !Path::new(value).is_absolute() {
            return Err(reinstall_error(
                format!(
                    "installed default profile field {field} must be absolute: observed={value}"
                ),
                repair_command,
            ));
        }
    }
    Ok(InstalledProfilePaths {
        firecracker_bin: toml_string(
            &table,
            "firecracker_bin",
            "installed default profile",
            repair_command,
        )?
        .to_owned(),
        firecracker_seccomp_filter: toml_string(
            &table,
            "firecracker_seccomp_filter",
            "installed default profile",
            repair_command,
        )?
        .to_owned(),
        jailer_bin: toml_string(
            &table,
            "jailer_bin",
            "installed default profile",
            repair_command,
        )?
        .to_owned(),
    })
}

struct InstalledProfilePaths {
    firecracker_bin: String,
    firecracker_seccomp_filter: String,
    jailer_bin: String,
}

fn verify_host_binaries_manifest(
    install_root: &Path,
    final_dir: &Path,
    profile: &InstalledProfilePaths,
    repair_command: &str,
) -> Result<(), FcError> {
    let path = install_root.join("artifacts/host-binaries.manifest.json");
    let manifest = HostBinariesManifest::read(&path).map_err(|err| {
        reinstall_error(
            format!("installed host-binaries manifest is unreadable: {err}"),
            repair_command,
        )
    })?;
    if manifest.binaries.len() != 5 {
        return Err(reinstall_error(
            format!(
                "installed host-binaries manifest must list exactly five host binaries: path={} observed={}",
                path.display(),
                manifest.binaries.len()
            ),
            repair_command,
        ));
    }
    if manifest.launch_material.len() != 1 {
        return Err(reinstall_error(
            format!(
                "installed host-binaries manifest must list exactly one launch material: path={} observed={}",
                path.display(),
                manifest.launch_material.len()
            ),
            repair_command,
        ));
    }
    require_host_binary(
        &manifest,
        HostBinaryName::Firecracker,
        Path::new(&profile.firecracker_bin),
        None,
        repair_command,
    )?;
    require_host_binary(
        &manifest,
        HostBinaryName::Jailer,
        Path::new(&profile.jailer_bin),
        None,
        repair_command,
    )?;
    let m80_sha256 = installed_binary_sha256(final_dir, "bin/m80", "m80 binary", repair_command)?;
    let jailer_harden_sha256 = installed_binary_sha256(
        final_dir,
        "bin/m80-jailer-harden",
        "m80-jailer-harden binary",
        repair_command,
    )?;
    let net_helper_sha256 = installed_binary_sha256(
        final_dir,
        "bin/m80-net-helper",
        "m80-net-helper binary",
        repair_command,
    )?;
    require_host_binary(
        &manifest,
        HostBinaryName::M80,
        &install_root.join("bin/m80"),
        Some(m80_sha256.as_str()),
        repair_command,
    )?;
    require_host_binary(
        &manifest,
        HostBinaryName::M80JailerHarden,
        &install_root.join("bin/m80-jailer-harden"),
        Some(jailer_harden_sha256.as_str()),
        repair_command,
    )?;
    require_host_binary(
        &manifest,
        HostBinaryName::M80NetHelper,
        &install_root.join("bin/m80-net-helper"),
        Some(net_helper_sha256.as_str()),
        repair_command,
    )?;
    require_launch_material(
        &manifest,
        HostLaunchMaterialName::FirecrackerSeccompFilter,
        Path::new(&profile.firecracker_seccomp_filter),
        repair_command,
    )?;
    Ok(())
}

fn installed_binary_sha256(
    root: &Path,
    relative: &str,
    label: &'static str,
    repair_command: &str,
) -> Result<String, FcError> {
    bundle::sha256_file(&root.join(relative)).map_err(|err| {
        reinstall_error(
            format!("installed {label} could not be hashed: {err}"),
            repair_command,
        )
    })
}

fn require_host_binary(
    manifest: &HostBinariesManifest,
    name: HostBinaryName,
    expected_path: &Path,
    expected_sha256: Option<&str>,
    repair_command: &str,
) -> Result<(), FcError> {
    let binary = unique_host_binary(manifest, name, repair_command)?;
    if binary.path != expected_path {
        return Err(reinstall_error(
            format!(
                "installed host-binaries manifest path mismatch for {}: expected={} observed={}",
                name.as_str(),
                expected_path.display(),
                binary.path.display()
            ),
            repair_command,
        ));
    }
    require_absolute_manifest_path(
        "host_binaries_manifest.binaries.path",
        &binary.path,
        repair_command,
    )?;
    require_sha256(
        "host_binaries_manifest.binaries.sha256",
        &binary.sha256,
        repair_command,
    )?;
    if let Some(expected_sha256) = expected_sha256 {
        if binary.sha256 != expected_sha256 {
            return Err(reinstall_error(
                format!(
                    "installed host-binaries manifest sha256 mismatch for {}: expected={} observed={}",
                    name.as_str(),
                    expected_sha256,
                    binary.sha256
                ),
                repair_command,
            ));
        }
    }
    require_nonempty(
        "host_binaries_manifest.binaries.version",
        &binary.version,
        repair_command,
    )
}

fn unique_host_binary<'a>(
    manifest: &'a HostBinariesManifest,
    name: HostBinaryName,
    repair_command: &str,
) -> Result<&'a HostBinaryEntry, FcError> {
    let mut matches = manifest
        .binaries
        .iter()
        .filter(|binary| binary.name == name);
    let Some(binary) = matches.next() else {
        return Err(reinstall_error(
            format!(
                "installed host-binaries manifest missing binary {}",
                name.as_str()
            ),
            repair_command,
        ));
    };
    if matches.next().is_some() {
        return Err(reinstall_error(
            format!(
                "installed host-binaries manifest duplicates binary {}",
                name.as_str()
            ),
            repair_command,
        ));
    }
    Ok(binary)
}

fn require_launch_material(
    manifest: &HostBinariesManifest,
    name: HostLaunchMaterialName,
    expected_path: &Path,
    repair_command: &str,
) -> Result<(), FcError> {
    let material = unique_launch_material(manifest, name, repair_command)?;
    if material.path != expected_path {
        return Err(reinstall_error(
            format!(
                "installed host-binaries manifest launch material path mismatch for {}: expected={} observed={}",
                name.as_str(),
                expected_path.display(),
                material.path.display()
            ),
            repair_command,
        ));
    }
    require_absolute_manifest_path(
        "host_binaries_manifest.launch_material.path",
        &material.path,
        repair_command,
    )?;
    require_sha256(
        "host_binaries_manifest.launch_material.sha256",
        &material.sha256,
        repair_command,
    )?;
    require_nonempty(
        "host_binaries_manifest.launch_material.version",
        &material.version,
        repair_command,
    )
}

fn unique_launch_material<'a>(
    manifest: &'a HostBinariesManifest,
    name: HostLaunchMaterialName,
    repair_command: &str,
) -> Result<&'a HostLaunchMaterialEntry, FcError> {
    let mut matches = manifest
        .launch_material
        .iter()
        .filter(|material| material.name == name);
    let Some(material) = matches.next() else {
        return Err(reinstall_error(
            format!(
                "installed host-binaries manifest missing launch material {}",
                name.as_str()
            ),
            repair_command,
        ));
    };
    if matches.next().is_some() {
        return Err(reinstall_error(
            format!(
                "installed host-binaries manifest duplicates launch material {}",
                name.as_str()
            ),
            repair_command,
        ));
    }
    Ok(material)
}

fn read_toml_table(
    path: &Path,
    label: &'static str,
    repair_command: &str,
) -> Result<toml::map::Map<String, toml::Value>, FcError> {
    let raw = fs::read_to_string(path).map_err(|source| {
        reinstall_error(
            format!(
                "{label} is unreadable: path={} source={source}",
                path.display()
            ),
            repair_command,
        )
    })?;
    match toml::from_str::<toml::Value>(&raw) {
        Ok(toml::Value::Table(table)) => Ok(table),
        Ok(_) => Err(reinstall_error(
            format!("{label} must be a TOML table: path={}", path.display()),
            repair_command,
        )),
        Err(source) => Err(reinstall_error(
            format!(
                "{label} TOML parse failed: path={} source={source}",
                path.display()
            ),
            repair_command,
        )),
    }
}

fn reject_unknown_keys(
    table: &toml::map::Map<String, toml::Value>,
    allowed: &[&str],
    label: &'static str,
    repair_command: &str,
) -> Result<(), FcError> {
    for key in table.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(reinstall_error(
                format!("{label} contains unknown key {key:?}"),
                repair_command,
            ));
        }
    }
    Ok(())
}

fn require_toml_string(
    table: &toml::map::Map<String, toml::Value>,
    field: &'static str,
    expected: &str,
    label: &'static str,
    repair_command: &str,
) -> Result<(), FcError> {
    let observed = toml_string(table, field, label, repair_command)?;
    if observed == expected {
        Ok(())
    } else {
        Err(reinstall_error(
            format!("{label} field {field} mismatch: expected={expected} observed={observed}"),
            repair_command,
        ))
    }
}

fn toml_string<'a>(
    table: &'a toml::map::Map<String, toml::Value>,
    field: &'static str,
    label: &'static str,
    repair_command: &str,
) -> Result<&'a str, FcError> {
    table
        .get(field)
        .and_then(toml::Value::as_str)
        .ok_or_else(|| {
            reinstall_error(
                format!("{label} field {field} must be a string"),
                repair_command,
            )
        })
}

fn require_absolute_manifest_path(
    field: &'static str,
    path: &Path,
    repair_command: &str,
) -> Result<(), FcError> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(reinstall_error(
            format!("{field} must be absolute: observed={}", path.display()),
            repair_command,
        ))
    }
}

fn require_sha256(field: &'static str, value: &str, repair_command: &str) -> Result<(), FcError> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(reinstall_error(
            format!("{field} must be a 64-hex sha256 digest"),
            repair_command,
        ))
    }
}

fn require_nonempty(field: &'static str, value: &str, repair_command: &str) -> Result<(), FcError> {
    if value.is_empty() {
        Err(reinstall_error(
            format!("{field} must not be empty"),
            repair_command,
        ))
    } else {
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
struct FileFingerprint {
    sha256: String,
    size_bytes: u64,
    mode: u32,
}

fn file_fingerprint(path: &Path, repair_command: &str) -> Result<FileFingerprint, FcError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| {
        reinstall_error(
            format!(
                "installed release path is unreadable: path={} source={source}",
                path.display()
            ),
            repair_command,
        )
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(reinstall_error(
            format!(
                "installed release path must be a regular file: path={}",
                path.display()
            ),
            repair_command,
        ));
    }
    Ok(FileFingerprint {
        sha256: bundle::sha256_file(path).map_err(|err| {
            reinstall_error(
                format!("installed release path could not be hashed: {err}"),
                repair_command,
            )
        })?,
        size_bytes: metadata.len(),
        mode: metadata.permissions().mode() & 0o777,
    })
}

fn repair_command(bundle_url: &str, install_root: &Path, final_dir: &Path) -> String {
    format!(
        "rm -rf -- {} && m80 install --bundle-url {} --install-root {}",
        shell_quote(&final_dir.display().to_string()),
        shell_quote(bundle_url),
        shell_quote(&install_root.display().to_string())
    )
}

pub(super) fn reinstall_error(reason: String, repair_command: &str) -> FcError {
    reinstall_error_with_field("install.reinstall", reason, repair_command)
}

fn reinstall_error_with_field(
    field: &'static str,
    reason: String,
    repair_command: &str,
) -> FcError {
    FcError::Config(ConfigError::InvalidValue {
        field,
        reason: format!("{reason}; repair_command={repair_command}"),
    })
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
