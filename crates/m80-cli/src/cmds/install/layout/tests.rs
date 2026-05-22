use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::super::SourceKind;
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

#[test]
fn official_release_missing_attestation_verifier_fails_before_staging() {
    let _guard = INSTALL_PREFLIGHT_ENV_LOCK.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let missing_gh = temp.path().join("missing-gh");
    let _env = AttestationGhEnv::set(&missing_gh);

    let err = install_bundle_layout(&official_release_plan(&install_root)).unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("release attestation verifier missing"),
        "{message}"
    );
    assert!(
        message.contains("Install or upgrade GitHub CLI with attestation support on Linux"),
        "{message}"
    );
    assert!(
        !install_root.exists(),
        "attestation verifier failure must happen before staging creates the install root"
    );
}

#[test]
fn official_release_too_old_attestation_verifier_fails_before_staging() {
    let _guard = INSTALL_PREFLIGHT_ENV_LOCK.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let fake_gh = write_fake_gh(
        temp.path(),
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then printf 'gh version 2.0.0\\n'; exit 0; fi\nprintf 'unknown command \"attestation\" for \"gh\"\\n' >&2\nexit 1\n",
    );
    let _env = AttestationGhEnv::set(&fake_gh);

    let err = install_bundle_layout(&official_release_plan(&install_root)).unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("release attestation verifier unsupported"),
        "{message}"
    );
    assert!(message.contains("gh version 2.0.0"), "{message}");
    assert!(
        message.contains("unknown command \"attestation\""),
        "{message}"
    );
    assert!(
        !install_root.exists(),
        "attestation verifier failure must happen before staging creates the install root"
    );
}

fn official_release_plan(install_root: &Path) -> InstallPlan {
    let bundle_url = crate::release_urls::release_asset_url("v0.0.0", "m80-linux-x86_64.tar.gz");
    InstallPlan {
        dry_run: false,
        install_root: install_root.display().to_string(),
        active_version_dir: Some(install_root.join("versions/v0.0.0").display().to_string()),
        active_pointer: install_root.join("active").display().to_string(),
        active_pointer_changed: false,
        repair_stale_install_lock: false,
        source: super::super::SourcePlan {
            kind: SourceKind::BundleUrl,
            selector: bundle_url.clone(),
            release_tag: Some("v0.0.0".to_owned()),
            bundle_url: Some(bundle_url),
        },
        bundle_url: Some(crate::release_urls::release_asset_url(
            "v0.0.0",
            "m80-linux-x86_64.tar.gz",
        )),
        default_profile: install_root
            .join("profiles/default.toml")
            .display()
            .to_string(),
        host_binaries_manifest: Some(
            install_root
                .join("versions/v0.0.0/artifacts/host-binaries.manifest.json")
                .display()
                .to_string(),
        ),
        profile_written: false,
        next_command: "m80 run -- echo hello".to_owned(),
        binary_version: "v0.0.0".to_owned(),
        binary_release_tag: Some("v0.0.0".to_owned()),
        version_status: "release".to_owned(),
    }
}

fn write_fake_gh(root: &Path, body: &str) -> PathBuf {
    let path = root.join("fake-gh");
    fs::write(&path, body).unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
    path
}

struct AttestationGhEnv {
    previous: Option<OsString>,
}

impl AttestationGhEnv {
    fn set(path: &Path) -> Self {
        let previous = std::env::var_os("M80_RELEASE_ATTESTATION_GH");
        std::env::set_var("M80_RELEASE_ATTESTATION_GH", path);
        Self { previous }
    }
}

impl Drop for AttestationGhEnv {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var("M80_RELEASE_ATTESTATION_GH", value),
            None => std::env::remove_var("M80_RELEASE_ATTESTATION_GH"),
        }
    }
}
