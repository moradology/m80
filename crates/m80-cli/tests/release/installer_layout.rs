use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use m80_image_manifest::{
    BuildReceipt, BuildReceiptArtifactKind, InstallProvenance, InstallProvenanceArtifact,
    InstallProvenanceRewrite, Manifest,
};
use serde_json::Value;

#[path = "../common/mod.rs"]
mod common;
#[path = "installer_layout/finalization.rs"]
mod finalization;
#[path = "installer_layout/fixture.rs"]
mod fixture;
#[path = "installer_layout/http_fixture.rs"]
mod http_fixture;
#[path = "installer_layout/path_canonicalization.rs"]
mod path_canonicalization;
#[path = "installer_layout/remote_fetch.rs"]
mod remote_fetch;

use common::m80;
use fixture::{
    read_repo_file, running_as_root, set_mode, sha256_hex, write_duplicate_path_bundle,
    write_release_bundle, write_release_bundle_with_hook, RELEASE_TAG,
};

const REQUIRED_INSTALLED_FILES: &[&str] = &[
    "bin/m80",
    "bin/m80-jailer-harden",
    "bin/m80-net-helper",
    "artifacts/vmlinux",
    "artifacts/output.ext4",
    "artifacts/output.ext4.manifest.json",
    "artifacts/output.ext4.build-receipt.json",
    "artifacts/m80-guestd",
    "install.sh",
    "bundle.json",
    "SHA256SUMS",
];

struct HostPrereqFixture {
    _temp: tempfile::TempDir,
    firecracker_bin: PathBuf,
    firecracker_seccomp_filter: PathBuf,
    jailer_bin: PathBuf,
}

impl HostPrereqFixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let firecracker_bin = bin_dir.join("firecracker");
        let jailer_bin = bin_dir.join("jailer");
        let firecracker_seccomp_filter = bin_dir.join("firecracker-seccomp-filter.bin");
        write_executable(
            &firecracker_bin,
            "#!/bin/sh\nprintf 'Firecracker v1.15.1\\n'\n",
        );
        write_executable(&jailer_bin, "#!/bin/sh\nprintf 'Jailer v1.15.1\\n'\n");
        fs::write(&firecracker_seccomp_filter, b"{\"seccomp_level\":2}\n").unwrap();

        Self {
            _temp: temp,
            firecracker_bin,
            firecracker_seccomp_filter,
            jailer_bin,
        }
    }

    fn apply(&self, cmd: &mut assert_cmd::Command) {
        clear_install_env(cmd);
        cmd.env("M80_FIRECRACKER_BIN", &self.firecracker_bin);
        cmd.env(
            "M80_FIRECRACKER_SECCOMP_FILTER",
            &self.firecracker_seccomp_filter,
        );
        cmd.env("M80_JAILER_BIN", &self.jailer_bin);
        cmd.env("M80_INSTALL_HOSTLESS_FIXTURE", "1");
    }
}

#[test]
fn install_bundle_layout_copies_verified_bundle_into_version_dir() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");

    let output = run_install(&bundle, &install_root, Some(&host), &[], &[]);

    assert!(
        output.status.success(),
        "install failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("installed bundle layout"), "{stdout}");
    assert!(stdout.contains("files_copied=12"), "{stdout}");
    assert!(stdout.contains("active_pointer_flipped=true"), "{stdout}");
    assert!(stdout.contains("profile_written=true"), "{stdout}");
    assert!(
        stdout.contains("preflight_gate=hostless_fixture"),
        "{stdout}"
    );
    assert!(
        stdout.contains("finalization_order=bundle_verification,host_prerequisite_verification,install_provenance,host_binaries_manifest,default_profile,preflight_smoke_gate,active_pointer_flip"),
        "{stdout}"
    );

    let version_dir = install_root.join("versions").join(&bundle.release_tag);
    assert!(
        stdout.contains(&format!("version_dir={}", version_dir.display())),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("install_root={}", install_root.display())),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("active_version_dir={}", version_dir.display())),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("bundle_url=file://{}", bundle.tarball.display())),
        "{stdout}"
    );
    let installed_m80 = install_root.join("bin/m80");
    assert!(
        stdout.contains(&format!("installed_m80_path={}", installed_m80.display())),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("installed_m80_version=m80 {}", bundle.release_tag)),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("active_bundle_path={}", version_dir.display())),
        "{stdout}"
    );
    assert_eq!(
        fs::read_link(&installed_m80).unwrap(),
        version_dir.join("bin/m80")
    );
    for relpath in REQUIRED_INSTALLED_FILES {
        assert!(
            version_dir.join(relpath).is_file(),
            "installed bundle missing {relpath}"
        );
    }
    let provenance_path = version_dir.join("artifacts/install-provenance.json");
    assert!(
        provenance_path.is_file(),
        "installer must emit installed provenance"
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        version_dir
    );

    let artifacts = version_dir.join("artifacts");
    let host_binaries_manifest = artifacts.join("host-binaries.manifest.json");
    assert!(
        host_binaries_manifest.is_file(),
        "installer must emit host-binaries manifest"
    );
    assert!(
        stdout.contains(&format!(
            "host_binaries_manifest={}",
            host_binaries_manifest.display()
        )),
        "{stdout}"
    );
    let profile_path = install_root.join("profiles/default.toml");
    assert!(
        profile_path.is_file(),
        "installer must write default profile"
    );
    assert!(
        stdout.contains(&format!("profile_path={}", profile_path.display())),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("default_profile={}", profile_path.display())),
        "{stdout}"
    );
    assert!(
        stdout.contains("host_prerequisite_status=passed:hostless_fixture"),
        "{stdout}"
    );
    assert!(
        stdout.contains("next_command=m80 run -- echo hello"),
        "{stdout}"
    );
    let profile = fs::read_to_string(&profile_path).unwrap();
    assert!(profile.contains("host_binaries_manifest = "), "{profile}");
    assert!(
        profile.contains(&host_binaries_manifest.display().to_string()),
        "{profile}"
    );
    assert!(profile.contains("jailer_harden_bin = "), "{profile}");
    assert!(
        profile.contains(
            &version_dir
                .join("bin/m80-jailer-harden")
                .display()
                .to_string()
        ),
        "{profile}"
    );
    assert!(profile.contains("net_helper_bin = "), "{profile}");
    assert!(
        profile.contains(&version_dir.join("bin/m80-net-helper").display().to_string()),
        "{profile}"
    );
    let config = fs::read_to_string(install_root.join("config.toml")).unwrap();
    assert!(config.contains("default_profile = 'default'"), "{config}");

    let manifest_path = artifacts.join("output.ext4.manifest.json");
    let manifest = Manifest::read(&manifest_path).unwrap();
    assert_eq!(manifest.kernel_image, artifacts.join("vmlinux"));
    assert_eq!(manifest.output_rootfs_image, artifacts.join("output.ext4"));
    assert_eq!(manifest.daemon_binary_path, artifacts.join("m80-guestd"));
    manifest.verify(&artifacts).unwrap();

    let receipt_path = artifacts.join("output.ext4.build-receipt.json");
    let receipt = BuildReceipt::read(&receipt_path).unwrap();
    assert_eq!(receipt.manifest_path, manifest_path);
    assert_eq!(receipt.manifest_sha256, sha256_hex(&manifest_path));
    assert_receipt_artifact(
        &receipt,
        BuildReceiptArtifactKind::KernelImage,
        &artifacts.join("vmlinux"),
    );
    assert_receipt_artifact(
        &receipt,
        BuildReceiptArtifactKind::OutputRootfsImage,
        &artifacts.join("output.ext4"),
    );
    assert_receipt_artifact(
        &receipt,
        BuildReceiptArtifactKind::DaemonBinaryPath,
        &artifacts.join("m80-guestd"),
    );

    let provenance = InstallProvenance::read(&provenance_path).unwrap();
    assert_eq!(provenance.release_tag.as_deref(), Some(RELEASE_TAG));
    assert_eq!(provenance.transforms.len(), 2);
    assert_rewrite_record(
        &provenance,
        InstallProvenanceArtifact::GuestManifest,
        "artifacts/output.ext4.manifest.json",
        &manifest_path,
    );
    assert_rewrite_record(
        &provenance,
        InstallProvenanceArtifact::BuildReceipt,
        "artifacts/output.ext4.build-receipt.json",
        &receipt_path,
    );
}

#[test]
fn install_json_success_uses_short_machine_summary_fields() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");

    let output = run_install_json(&bundle, &install_root, Some(&host));

    assert!(
        output.status.success(),
        "install --json failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let data = &value["data"];
    let version_dir = install_root.join("versions").join(&bundle.release_tag);
    let default_profile = install_root.join("profiles/default.toml");
    let host_binaries_manifest = version_dir.join("artifacts/host-binaries.manifest.json");

    assert_eq!(data["state"], "installed");
    assert_eq!(data["release_tag"], bundle.release_tag);
    assert_eq!(data["install_root"], install_root.display().to_string());
    assert_eq!(
        data["active_version_dir"],
        version_dir.display().to_string()
    );
    assert_eq!(data["version_dir"], version_dir.display().to_string());
    assert_eq!(
        data["bundle_url"],
        format!("file://{}", bundle.tarball.display())
    );
    assert_eq!(
        data["installed_m80_path"],
        install_root.join("bin/m80").display().to_string()
    );
    assert_eq!(
        data["installed_m80_version"],
        format!("m80 {}", bundle.release_tag)
    );
    assert_eq!(
        data["active_bundle_path"],
        version_dir.display().to_string()
    );
    assert_eq!(
        data["default_profile"],
        default_profile.display().to_string()
    );
    assert_eq!(
        data["host_binaries_manifest"],
        host_binaries_manifest.display().to_string()
    );
    assert_eq!(data["host_prerequisite_status"], "passed:hostless_fixture");
    assert_eq!(data["next_command"], "m80 run -- echo hello");
    assert_eq!(data["active_pointer_flipped"], true);
    assert_eq!(data["profile_written"], true);
}

#[test]
fn install_bundle_layout_fails_when_older_m80_shadows_installed_path() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let previous = seed_previous_active_install(&install_root);
    fs::create_dir_all(install_root.join("bin")).unwrap();
    symlink(previous.join("bin/m80"), install_root.join("bin/m80")).unwrap();
    let old_bin = install_temp.path().join("old-bin");
    fs::create_dir(&old_bin).unwrap();
    write_executable(
        &old_bin.join("m80"),
        "#!/bin/sh\nprintf 'm80 v0.0.OLD\\n'\n",
    );
    let path = path_with_dirs([old_bin.clone(), install_root.join("bin")]);

    let output = run_install(
        &bundle,
        &install_root,
        Some(&host),
        &[("PATH", path.to_str().unwrap())],
        &[],
    );

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("PATH handoff failed"), "{stderr}");
    assert!(
        stderr.contains(&format!(
            "expected {}",
            install_root.join("bin/m80").display()
        )),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!(
            "export PATH={}:$PATH",
            install_root.join("bin").display()
        )),
        "{stderr}"
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        previous
    );
    assert_eq!(
        fs::read_link(install_root.join("bin/m80")).unwrap(),
        previous.join("bin/m80")
    );
}

#[test]
fn install_bundle_layout_fails_with_repair_when_m80_is_not_on_path() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let previous = seed_previous_active_install(&install_root);

    let output = run_install(
        &bundle,
        &install_root,
        Some(&host),
        &[("PATH", "/usr/bin:/bin")],
        &[],
    );

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("PATH handoff failed"), "{stderr}");
    assert!(stderr.contains("no m80 on PATH"), "{stderr}");
    assert!(
        stderr.contains(&format!(
            "export PATH={}:$PATH",
            install_root.join("bin").display()
        )),
        "{stderr}"
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        previous
    );
    assert!(install_root.join("bin/m80").symlink_metadata().is_err());
}

#[test]
fn install_bundle_layout_rejects_existing_command_directory_before_handoff_mutation() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let previous = seed_previous_active_install(&install_root);
    let bin_dir = install_root.join("bin");
    fs::create_dir_all(bin_dir.join("m80")).unwrap();

    let output = run_install(&bundle, &install_root, Some(&host), &[], &[]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("already exists but is not a file or symlink"),
        "{stderr}"
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        previous
    );
    assert!(bin_dir.join("m80").is_dir());
    assert!(
        fs::read_dir(&bin_dir).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".m80.install")),
        "handoff rejection must not leave temp links"
    );
}

#[test]
fn install_bundle_layout_explicit_bin_dir_override_controls_handoff_path() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let bin_dir = install_temp.path().join("explicit-bin");
    let path = path_with_dirs([bin_dir.clone()]);

    let mut command = m80();
    command.args([
        "install",
        "--bundle-url",
        &format!("file://{}", bundle.tarball.display()),
        "--install-root",
        install_root.to_str().unwrap(),
        "--bin-dir",
        bin_dir.to_str().unwrap(),
    ]);
    clear_install_env(&mut command);
    command.env("PATH", path);
    host.apply(&mut command);
    let output = command.output().unwrap();

    assert!(
        output.status.success(),
        "install failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let installed_m80 = bin_dir.join("m80");
    let version_dir = install_root.join("versions").join(&bundle.release_tag);
    assert!(
        stdout.contains(&format!("installed_m80_path={}", installed_m80.display())),
        "{stdout}"
    );
    assert_eq!(
        fs::read_link(installed_m80).unwrap(),
        version_dir.join("bin/m80")
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        version_dir
    );
}

#[test]
fn install_bundle_layout_missing_required_bundle_file_fails_before_activation() {
    let bundle = write_release_bundle(Some("bin/m80-net-helper"));
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let previous = seed_previous_active_install(&install_root);

    let output = m80()
        .args([
            "install",
            "--bundle-url",
            &format!("file://{}", bundle.tarball.display()),
            "--install-root",
            install_root.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("required artifact missing") && stderr.contains("bin/m80-net-helper"),
        "unexpected stderr: {stderr}"
    );
    assert!(
        !stderr.contains("next_command="),
        "failure text must not print a success next step: {stderr}"
    );
    assert!(
        fs::read_link(install_root.join("active")).unwrap() == previous,
        "missing bundle file must leave previous active pointer selected"
    );
    assert!(
        !install_root.join("versions").join(RELEASE_TAG).exists(),
        "missing bundle file must not publish a version dir"
    );
}

#[test]
fn install_bundle_layout_duplicate_bundle_path_fails_before_activation() {
    let temp = tempfile::tempdir().unwrap();
    let tarball = write_duplicate_path_bundle(temp.path());
    let install_root = temp.path().join("install-root");

    let output = m80()
        .args([
            "install",
            "--bundle-url",
            &format!("file://{}", tarball.display()),
            "--install-root",
            install_root.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(6));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("bundle duplicate path: bin/m80"),
        "unexpected stderr: {stderr}"
    );
    assert!(
        !install_root.join("active").exists(),
        "duplicate bundle path must not switch active pointer"
    );
}

#[test]
fn install_bundle_layout_symlink_payload_fails_before_activation() {
    let bundle = write_release_bundle_with_hook(None, |src| {
        fs::remove_file(src.join("bin/m80-net-helper")).unwrap();
        symlink("m80", src.join("bin/m80-net-helper")).unwrap();
    });
    assert_malformed_bundle_fails_before_activation(&bundle, "non-regular file");
}

#[test]
fn install_bundle_layout_hardlink_payload_fails_before_activation() {
    let bundle = write_release_bundle_with_hook(None, |src| {
        fs::remove_file(src.join("bin/m80-net-helper")).unwrap();
        fs::hard_link(src.join("bin/m80"), src.join("bin/m80-net-helper")).unwrap();
    });
    assert_malformed_bundle_fails_before_activation(&bundle, "must not be a hardlink");
}

#[test]
fn install_bundle_layout_directory_payload_fails_before_activation() {
    let bundle = write_release_bundle_with_hook(None, |src| {
        fs::remove_file(src.join("bin/m80-net-helper")).unwrap();
        fs::create_dir(src.join("bin/m80-net-helper")).unwrap();
    });
    assert_malformed_bundle_fails_before_activation(&bundle, "directory where file expected");
}

#[test]
fn install_bundle_layout_device_like_payload_fails_before_activation() {
    let bundle = write_release_bundle_with_hook(None, |src| {
        fs::remove_file(src.join("artifacts/vmlinux")).unwrap();
        run_checked(
            StdCommand::new("mkfifo").arg(src.join("artifacts/vmlinux")),
            "mkfifo",
        );
    });
    assert_malformed_bundle_fails_before_activation(&bundle, "non-regular file");
}

#[test]
fn install_bundle_layout_bad_payload_mode_fails_before_activation() {
    let bundle = write_release_bundle_with_hook(None, |src| {
        set_mode(&src.join("artifacts/vmlinux"), 0o755);
    });
    assert_malformed_bundle_fails_before_activation(&bundle, "bundle file mode mismatch");
}

#[test]
fn install_bundle_layout_permission_failure_leaves_active_state_untouched() {
    if running_as_root() {
        return;
    }

    let bundle = write_release_bundle(None);
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    fs::create_dir(&install_root).unwrap();
    set_mode(&install_root, 0o500);

    let output = m80()
        .args([
            "install",
            "--bundle-url",
            &format!("file://{}", bundle.tarball.display()),
            "--install-root",
            install_root.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    set_mode(&install_root, 0o700);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        !install_root.join("active").exists(),
        "permission failure must not switch active pointer"
    );
    assert!(
        !install_root.join("versions").join(RELEASE_TAG).exists(),
        "permission failure must not publish a version dir"
    );
}

#[test]
fn install_bundle_layout_dry_run_never_reads_or_writes_bundle_layout() {
    let bundle = write_release_bundle(None);
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");

    let output = m80()
        .args([
            "install",
            "--bundle-url",
            &format!("file://{}", bundle.tarball.display()),
            "--install-root",
            install_root.to_str().unwrap(),
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "install dry-run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("install dry-run"), "{stdout}");
    assert!(stdout.contains("writes=none"), "{stdout}");
    assert!(stdout.contains("active_pointer_changed=false"), "{stdout}");
    assert!(stdout.contains("profile_written=false"), "{stdout}");
    assert!(
        stdout.contains("active_version_dir=<resolved after bundle verification>"),
        "{stdout}"
    );
    assert!(
        stdout.contains("host_binaries_manifest=<resolved after bundle verification>"),
        "{stdout}"
    );
    assert!(
        stdout.contains("next_command=m80 run -- echo hello"),
        "{stdout}"
    );
    assert!(
        !install_root.exists(),
        "dry-run must not create install root {}",
        install_root.display()
    );
}

#[test]
fn install_state_lock_blocks_second_writer_before_staging_or_activation() {
    let bundle = write_release_bundle(None);
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let previous = seed_previous_active_install(&install_root);
    write_install_lock(
        &install_root,
        std::process::id(),
        Some("v-lock-owner"),
        current_proc_start_ticks(),
    );

    let output = run_install(&bundle, &install_root, None, &[], &[]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("install.lock"), "{stderr}");
    assert!(
        stderr.contains(&format!("owner_pid={}", std::process::id())),
        "{stderr}"
    );
    assert!(stderr.contains("--repair-stale-install-lock"), "{stderr}");
    assert!(
        !install_root.join(".staging").exists(),
        "lock contention must fail before staging"
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        previous
    );
}

#[test]
fn stale_install_state_lock_requires_explicit_repair_flag() {
    let bundle = write_release_bundle(None);
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let previous = seed_previous_active_install(&install_root);
    let lock_path = write_install_lock(&install_root, 999_999_999, Some("v-stale"), 0);

    let output = run_install(&bundle, &install_root, None, &[], &[]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("install.lock"), "{stderr}");
    assert!(stderr.contains("owner_pid=999999999"), "{stderr}");
    assert!(stderr.contains("--repair-stale-install-lock"), "{stderr}");
    assert!(
        lock_path.exists(),
        "stale lock must remain without repair flag"
    );
    assert!(
        !install_root.join(".staging").exists(),
        "stale lock refusal must fail before staging"
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        previous
    );
}

#[test]
fn repair_stale_install_state_lock_rejects_unreadable_lock_record() {
    let bundle = write_release_bundle(None);
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let previous = seed_previous_active_install(&install_root);
    let lock_path = write_raw_install_lock(&install_root, b"{");

    let mut command = m80();
    command.args([
        "install",
        "--bundle-url",
        &format!("file://{}", bundle.tarball.display()),
        "--install-root",
        install_root.to_str().unwrap(),
        "--repair-stale-install-lock",
    ]);
    clear_install_env(&mut command);
    let output = command.output().unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("install.lock"), "{stderr}");
    assert!(stderr.contains("owner=<unreadable>"), "{stderr}");
    assert!(stderr.contains("--repair-stale-install-lock"), "{stderr}");
    assert!(
        lock_path.exists(),
        "unreadable lock records must fail closed"
    );
    assert!(
        !install_root.join(".staging").exists(),
        "unreadable lock refusal must fail before staging"
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        previous
    );
}

#[test]
fn repair_stale_install_state_lock_ignores_reused_pid() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let lock_path = write_install_lock(&install_root, std::process::id(), Some("v-reused-pid"), 0);

    let mut command = m80();
    command.args([
        "install",
        "--bundle-url",
        &format!("file://{}", bundle.tarball.display()),
        "--install-root",
        install_root.to_str().unwrap(),
        "--repair-stale-install-lock",
    ]);
    clear_install_env(&mut command);
    command.env("PATH", path_with_install_bin_first(&install_root));
    host.apply(&mut command);
    let output = command.output().unwrap();

    assert!(
        output.status.success(),
        "reused-pid stale lock repair failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !lock_path.exists(),
        "successful install must release repaired lock"
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        install_root.join("versions").join(&bundle.release_tag)
    );
}

#[test]
fn repair_stale_install_state_lock_then_installs_normally() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let lock_path = write_install_lock(&install_root, 999_999_999, Some("v-stale"), 0);

    let mut command = m80();
    command.args([
        "install",
        "--bundle-url",
        &format!("file://{}", bundle.tarball.display()),
        "--install-root",
        install_root.to_str().unwrap(),
        "--repair-stale-install-lock",
    ]);
    clear_install_env(&mut command);
    command.env("PATH", path_with_install_bin_first(&install_root));
    host.apply(&mut command);
    let output = command.output().unwrap();

    assert!(
        output.status.success(),
        "stale lock repair install failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert!(
        !lock_path.exists(),
        "successful install must release install-state lock"
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        install_root.join("versions").join(&bundle.release_tag)
    );
}

fn run_install(
    bundle: &fixture::ReleaseBundleFixture,
    install_root: &Path,
    host: Option<&HostPrereqFixture>,
    envs: &[(&str, &str)],
    env_removals: &[&str],
) -> std::process::Output {
    run_install_url(
        &format!("file://{}", bundle.tarball.display()),
        install_root,
        host,
        envs,
        env_removals,
    )
}

fn run_install_json(
    bundle: &fixture::ReleaseBundleFixture,
    install_root: &Path,
    host: Option<&HostPrereqFixture>,
) -> std::process::Output {
    let mut command = m80();
    command.args([
        "--json",
        "install",
        "--bundle-url",
        &format!("file://{}", bundle.tarball.display()),
        "--install-root",
        install_root.to_str().unwrap(),
    ]);
    clear_install_env(&mut command);
    command.env("PATH", path_with_install_bin_first(install_root));
    if let Some(host) = host {
        host.apply(&mut command);
    }
    command.output().unwrap()
}

fn run_install_url(
    bundle_url: &str,
    install_root: &Path,
    host: Option<&HostPrereqFixture>,
    envs: &[(&str, &str)],
    env_removals: &[&str],
) -> std::process::Output {
    let mut command = m80();
    command.args([
        "install",
        "--bundle-url",
        bundle_url,
        "--install-root",
        install_root.to_str().unwrap(),
    ]);
    clear_install_env(&mut command);
    command.env("PATH", path_with_install_bin_first(install_root));
    for key in env_removals {
        command.env_remove(key);
    }
    if let Some(host) = host {
        host.apply(&mut command);
    }
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().unwrap()
}

pub(super) fn path_with_install_bin_first(install_root: &Path) -> std::ffi::OsString {
    path_with_dirs([install_root.join("bin")])
}

fn path_with_dirs(leading: impl IntoIterator<Item = PathBuf>) -> std::ffi::OsString {
    let mut paths = leading.into_iter().collect::<Vec<_>>();
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(paths).unwrap()
}

fn clear_install_env(command: &mut assert_cmd::Command) {
    for key in [
        "M80_CGROUP_MODE",
        "M80_JAIL_UID",
        "M80_JAIL_GID",
        "M80_EXPECTED_CONCURRENT_VMS",
        "M80_FIRECRACKER_BIN",
        "M80_FIRECRACKER_VERSION",
        "M80_FIRECRACKER_SECCOMP_FILTER",
        "M80_JAILER_BIN",
        "M80_JAILER_HARDEN_BIN",
        "M80_NET_HELPER_BIN",
        "M80_INSTALL_HOSTLESS_FIXTURE",
        "M80_INSTALL_INJECT_INTERRUPTION_AFTER_PROFILE",
        "M80_INSTALL_INJECT_PROOF_CACHE_WRITE_FAILURE",
        "M80_INSTALL_INJECT_PROOF_CACHE_DIGEST_FAILURE",
        "M80_INSTALL_INJECT_PROOF_CACHE_MODE_FAILURE",
        "M80_RELEASE_ATTESTATION_GH",
    ] {
        command.env_remove(key);
    }
}

fn seed_previous_active_install(install_root: &Path) -> PathBuf {
    let previous = install_root.join("versions/v-previous");
    fs::create_dir_all(&previous).unwrap();
    fs::write(previous.join("marker"), b"previous").unwrap();
    symlink(&previous, install_root.join("active")).unwrap();
    previous
}

fn write_install_lock(
    install_root: &Path,
    owner_pid: u32,
    resolved_tag: Option<&str>,
    owner_proc_start_ticks: u64,
) -> PathBuf {
    fs::create_dir_all(install_root).unwrap();
    let lock_path = install_root.join(".install-state.lock");
    let resolved_tag = resolved_tag
        .map(|tag| format!("\"{tag}\""))
        .unwrap_or_else(|| "null".to_owned());
    fs::write(
        &lock_path,
        format!(
            "{{\"owner_pid\":{owner_pid},\"command\":\"m80 install --bundle-url file://fixture\",\"resolved_tag\":{resolved_tag},\"started_at_unix_seconds\":1,\"owner_proc_start_ticks\":{owner_proc_start_ticks}}}\n"
        ),
    )
    .unwrap();
    lock_path
}

fn current_proc_start_ticks() -> u64 {
    let stat = fs::read_to_string(format!("/proc/{}/stat", std::process::id())).unwrap();
    let after_comm = stat.rsplit_once(") ").unwrap().1;
    after_comm
        .split_whitespace()
        .nth(19)
        .unwrap()
        .parse()
        .unwrap()
}

fn write_raw_install_lock(install_root: &Path, contents: &[u8]) -> PathBuf {
    fs::create_dir_all(install_root).unwrap();
    let lock_path = install_root.join(".install-state.lock");
    fs::write(&lock_path, contents).unwrap();
    lock_path
}

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    set_mode(path, 0o755);
}

fn assert_malformed_bundle_fails_before_activation(
    bundle: &fixture::ReleaseBundleFixture,
    expected_stderr: &str,
) {
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let previous = seed_previous_active_install(&install_root);

    let output = run_install(bundle, &install_root, None, &[], &[]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains(expected_stderr),
        "stderr missing {expected_stderr:?}: {stderr}"
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        previous,
        "malformed bundle must leave previous active pointer selected"
    );
    assert!(
        !install_root
            .join("versions")
            .join(&bundle.release_tag)
            .exists(),
        "malformed bundle must not publish a version dir"
    );
}

fn run_checked(cmd: &mut StdCommand, label: &str) {
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_receipt_artifact(
    receipt: &BuildReceipt,
    kind: BuildReceiptArtifactKind,
    installed_path: &Path,
) {
    let artifact = receipt
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == kind)
        .unwrap_or_else(|| panic!("missing receipt artifact {kind:?}"));
    assert_eq!(artifact.path, installed_path);
    assert_eq!(artifact.sha256, sha256_hex(installed_path));
}

fn assert_rewrite_record(
    provenance: &InstallProvenance,
    artifact: InstallProvenanceArtifact,
    source_name: &str,
    installed_path: &Path,
) {
    let transform = provenance
        .transforms
        .iter()
        .find(|transform| transform.artifact == artifact)
        .unwrap_or_else(|| panic!("missing provenance transform for {artifact:?}"));
    assert_eq!(transform.source_path, PathBuf::from(source_name));
    assert_eq!(transform.installed_path, installed_path);
    assert_eq!(
        transform.rewrite,
        InstallProvenanceRewrite::InstallPathRewrite
    );
    assert_eq!(transform.installed_sha256, sha256_hex(installed_path));
    assert_ne!(transform.source_sha256, transform.installed_sha256);
}
