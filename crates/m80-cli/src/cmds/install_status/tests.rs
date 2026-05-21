use std::path::PathBuf;

use m80_firecracker::{ConfigFilePaths, ConfigSource};

use super::*;
use crate::install_state::{
    ActivePointerReport, ActivePointerStatus, InstallConfigReport, InstallMetadataReport,
    InstallProfileReport, InstallStatePaths, InstallStateReport, MetadataFileReport,
    MetadataFileStatus,
};

#[test]
fn json_output_reports_active_install_paths() {
    let output = InstallStatusOutput::from_report(&active_report());
    let rendered = render_json(&output);
    let parsed: serde_json::Value =
        serde_json::from_str(&rendered).expect("install-status JSON should parse");
    let data = &parsed["data"];

    assert_eq!(data["schema_version"], 1);
    assert_eq!(data["status"], "healthy_active_release");
    assert_eq!(data["active"]["release_tag"], "v1.2.3");
    assert_eq!(data["active"]["install_dir"], "/opt/m80/versions/v1.2.3");
    assert_eq!(data["selected_profile"]["name"], "default");
    assert_eq!(data["selected_config"]["default_profile"], "default");
    assert_eq!(
        data["metadata"]["bundle_metadata"]["path"],
        "/opt/m80/versions/v1.2.3/bundle.json"
    );
    assert_eq!(
        data["metadata"]["host_binaries_manifest"]["path"],
        "/opt/m80/versions/v1.2.3/artifacts/host-binaries.manifest.json"
    );
    assert_eq!(
        data["metadata"]["install_provenance"]["path"],
        "/opt/m80/versions/v1.2.3/artifacts/install-provenance.json"
    );
    assert_eq!(
        data["metadata"]["proof_cache_manifest"]["path"],
        "/opt/m80/versions/v1.2.3/artifacts/release-proof-cache/manifest.json"
    );
    assert_eq!(data["next_action"]["kind"], "ready");
}

#[test]
fn human_output_reports_active_install_paths() {
    let output = InstallStatusOutput::from_report(&active_report());
    let rendered = render_human(&output);

    assert!(rendered.contains("status=healthy_active_release"));
    assert!(rendered.contains("active_release_tag=v1.2.3"));
    assert!(rendered.contains("active_install_dir=/opt/m80/versions/v1.2.3"));
    assert!(rendered.contains("selected_profile=default"));
    assert!(rendered.contains("selected_config_default_profile=default"));
    assert!(rendered.contains("bundle_metadata_path=/opt/m80/versions/v1.2.3/bundle.json"));
    assert!(rendered.contains(
        "host_binaries_manifest_path=/opt/m80/versions/v1.2.3/artifacts/host-binaries.manifest.json"
    ));
    assert!(rendered.contains(
        "install_provenance_path=/opt/m80/versions/v1.2.3/artifacts/install-provenance.json"
    ));
    assert!(rendered.contains(
        "proof_cache_manifest_path=/opt/m80/versions/v1.2.3/artifacts/release-proof-cache/manifest.json"
    ));
    assert!(rendered.contains("next_action=installed release is ready"));
    assert!(rendered.contains("next_action_command=m80 run -- echo hello"));
}

#[test]
fn human_output_for_missing_install_has_next_action() {
    let mut report = active_report();
    report.state = InstallStateKind::MissingActivePointer;
    report.active_pointer.status = ActivePointerStatus::Missing;
    report.active_pointer.release_tag = None;
    report.active_pointer.version_dir = None;
    report.metadata = None;

    let rendered = render_human(&InstallStatusOutput::from_report(&report));

    assert!(rendered.contains("status=missing_active_pointer"));
    assert!(rendered.contains("bundle_metadata_status=unavailable"));
    assert!(rendered.contains("proof_cache_manifest_status=unavailable"));
    assert!(rendered.contains("next_action=install a release"));
    assert!(rendered.contains(
        "next_action_command=curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh"
    ));
}

#[test]
fn reinstall_action_uses_safe_pinned_release_tag() {
    let mut report = active_report();
    report.state = InstallStateKind::TamperedProofCache;

    let output = InstallStatusOutput::from_report(&report);

    assert_eq!(
        output.next_action.command.as_deref(),
        Some(
            "curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.3/install.sh | sudo sh"
        )
    );
}

#[test]
fn install_status_args_use_host_default_paths() {
    let paths = InstallStatePaths::host("/opt/m80");
    let default_paths = ConfigFilePaths {
        system: Some(PathBuf::from("/etc/m80/config.toml")),
        system_drop_in_dir: Some(PathBuf::from("/etc/m80/config.d")),
        user: std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join(".config/m80/config.toml")),
        user_drop_in_dir: std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join(".config/m80/config.d")),
    };

    assert_eq!(paths.config_paths, default_paths);
}

fn active_report() -> InstallStateReport {
    let version_dir = PathBuf::from("/opt/m80/versions/v1.2.3");
    let artifacts_dir = version_dir.join("artifacts");
    InstallStateReport {
        state: InstallStateKind::HealthyActiveRelease,
        install_root: PathBuf::from("/opt/m80"),
        active_pointer: ActivePointerReport {
            path: PathBuf::from("/opt/m80/active"),
            target: Some(version_dir.clone()),
            version_dir: Some(version_dir.clone()),
            release_tag: Some("v1.2.3".to_owned()),
            status: ActivePointerStatus::Live,
        },
        config: InstallConfigReport {
            system_path: Some(PathBuf::from("/etc/m80/config.toml")),
            system_drop_in_dir: Some(PathBuf::from("/etc/m80/config.d")),
            user_path: None,
            user_drop_in_dir: None,
            default_profile: Some("default".to_owned()),
            default_profile_source: Some(ConfigSource::SystemFile),
            explicit_override: false,
        },
        profile: Some(InstallProfileReport {
            name: "default".to_owned(),
            selection_source: ConfigSource::SystemFile,
            body_source: "system_file",
            file_path: Some(PathBuf::from("/etc/m80/profiles/default.toml")),
            artifact_dir: Some(artifacts_dir.clone()),
            version_dir: Some(version_dir.clone()),
            release_tag: Some("v1.2.3".to_owned()),
            m80_version: Some("1.2.3".to_owned()),
        }),
        metadata: Some(InstallMetadataReport {
            version_dir: version_dir.clone(),
            bundle_metadata: metadata_file(version_dir.join("bundle.json")),
            install_provenance: metadata_file(artifacts_dir.join("install-provenance.json")),
            host_binaries_manifest: metadata_file(
                artifacts_dir.join("host-binaries.manifest.json"),
            ),
            proof_cache_manifest: metadata_file(
                artifacts_dir.join("release-proof-cache/manifest.json"),
            ),
            bundle: None,
            provenance: None,
            host_binaries: None,
            proof_cache: None,
        }),
        diagnostics: Vec::new(),
    }
}

fn metadata_file(path: PathBuf) -> MetadataFileReport {
    MetadataFileReport {
        path,
        status: MetadataFileStatus::Present,
        sha256: Some("0".repeat(64)),
    }
}
