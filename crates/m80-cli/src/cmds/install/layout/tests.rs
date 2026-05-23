use std::path::{Path, PathBuf};

use super::source::stage_bundle_source;
use super::*;

#[test]
fn default_install_uses_host_selector_paths() {
    let paths = install_selector_paths(Path::new("/opt/m80"));

    assert_eq!(paths.profile_dir, PathBuf::from("/etc/m80/profiles"));
    assert_eq!(paths.config_path, PathBuf::from("/etc/m80/config.toml"));
}

#[test]
fn install_root_override_uses_root_local_selector_paths() {
    let install_root = Path::new("/tank/tmp/m80-proof/install-root");
    let paths = install_selector_paths(install_root);

    assert_eq!(paths.profile_dir, install_root.join("profiles"));
    assert_eq!(paths.config_path, install_root.join("config.toml"));
}

#[test]
fn run_smoke_command_uses_installed_binary_and_public_echo_probe() {
    let final_dir = Path::new("/opt/m80/versions/v1.2.3");

    let command = process_smoke_command(final_dir);

    assert_eq!(
        command,
        [
            "/opt/m80/versions/v1.2.3/bin/m80",
            "run",
            "--",
            "echo",
            "hello"
        ]
    );
}

#[test]
fn local_file_url_requires_absolute_path() {
    let tmp = tempfile::tempdir().unwrap();
    let err = stage_bundle_source("file://relative.tar.gz", tmp.path()).unwrap_err();
    assert!(
        err.to_string().contains("absolute local path"),
        "unexpected error: {err}"
    );
}

#[test]
fn release_material_install_summary_initializes_proof_cache_fields() {
    let temp = tempfile::tempdir().unwrap();
    let final_dir = temp.path().join("versions/v0.0.0");
    let verification = release_material::ReleaseVerificationSummary {
        release_tag: "v0.0.0".to_owned(),
        repository: "moradology/m80".to_owned(),
        target: "linux-x86_64".to_owned(),
        bundle_asset: "m80-linux-x86_64.tar.gz".to_owned(),
        bundle_url:
            "https://github.com/moradology/m80/releases/download/v0.0.0/m80-linux-x86_64.tar.gz"
                .to_owned(),
        bundle_sha256: "a".repeat(64),
        install_sh_sha256: "b".repeat(64),
        public_sha256s_sha256: "c".repeat(64),
        asset_index_sha256: "d".repeat(64),
        predicate_sha256: "e".repeat(64),
        attestation_signer: "moradology/m80/.github/workflows/release-artifacts.yml".to_owned(),
        attestation_issuer: "https://token.actions.githubusercontent.com".to_owned(),
        attestation_keyset_id: "github-actions-oidc:m80-release-v1".to_owned(),
        source_commit: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        release_integrity_schema_version: 1,
    };

    let summary = ReleaseMaterialInstallSummary::from_verification(&verification, &final_dir);

    assert_eq!(summary.release_tag, "v0.0.0");
    assert_eq!(summary.bundle_asset, "m80-linux-x86_64.tar.gz");
    assert_eq!(summary.install_sh_sha256, "b".repeat(64));
    assert_eq!(summary.public_sha256s_sha256, "c".repeat(64));
    assert_eq!(summary.asset_index_sha256, "d".repeat(64));
    assert_eq!(
        summary.proof_cache_destination,
        final_dir
            .join("artifacts/release-proof-cache")
            .display()
            .to_string()
    );
    assert!(!summary.proof_cache_written);
    assert!(
        !final_dir.join("artifacts/release-proof-cache").exists(),
        "diagnostics reserve the proof-cache destination but do not write cache material yet"
    );
}
