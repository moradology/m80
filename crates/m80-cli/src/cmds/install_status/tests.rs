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
    assert_eq!(data["proof_cache"]["status"], "available");
    assert_eq!(
        data["proof_cache"]["cache_dir"],
        "/opt/m80/versions/v1.2.3/artifacts/release-proof-cache"
    );
    assert_eq!(data["proof_cache"]["manifest_digest"], "1".repeat(64));
    assert_eq!(
        data["proof_cache"]["materials"][0]["role"],
        "integrity_predicate"
    );
    assert_eq!(
        data["proof_cache"]["materials"][0]["sha256"],
        "2".repeat(64)
    );
    assert_eq!(
        data["proof_cache"]["trust_policy"]["sha256"],
        "3".repeat(64)
    );
    assert_eq!(data["mismatches"].as_array().unwrap().len(), 0);
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
    assert!(rendered.contains("proof_cache_status=available"));
    assert!(rendered.contains("proof_cache_manifest_digest=1111111111111111111111111111111111111111111111111111111111111111"));
    assert!(rendered.contains("proof_cache_material_0_role=integrity_predicate"));
    assert!(rendered.contains("proof_cache_material_0_sha256=2222222222222222222222222222222222222222222222222222222222222222"));
    assert!(rendered.contains("proof_cache_trust_policy_sha256=3333333333333333333333333333333333333333333333333333333333333333"));
    assert!(rendered.contains("next_action=installed release is ready"));
    assert!(rendered.contains("next_action_command=m80 run -- echo hello"));
}

#[test]
fn stale_profile_output_names_expected_and_observed_release_targets() {
    let mut report = active_report();
    let active_dir = PathBuf::from("/opt/m80/versions/v1.2.4");
    report.state = InstallStateKind::StaleProfileTarget;
    report.active_pointer.target = Some(active_dir.clone());
    report.active_pointer.version_dir = Some(active_dir);
    report.active_pointer.release_tag = Some("v1.2.4".to_owned());

    let output = InstallStatusOutput::from_report(&report);
    let rendered = render_json(&output);
    let parsed: serde_json::Value =
        serde_json::from_str(&rendered).expect("install-status JSON should parse");
    let mismatch = &parsed["data"]["mismatches"][0];

    assert_eq!(mismatch["code"], "stale_profile_target");
    assert_eq!(mismatch["expected_tag"], "v1.2.4");
    assert_eq!(mismatch["observed_tag"], "v1.2.3");
    assert_eq!(mismatch["expected_path"], "/opt/m80/versions/v1.2.4");
    assert_eq!(mismatch["observed_path"], "/opt/m80/versions/v1.2.3");

    let human = render_human(&output);
    assert!(human.contains("mismatch_0_code=stale_profile_target"));
    assert!(human.contains("mismatch_0_expected_tag=v1.2.4"));
    assert!(human.contains("mismatch_0_observed_tag=v1.2.3"));
}

#[test]
fn missing_selected_profile_output_names_unresolved_default_profile() {
    let mut report = active_report();
    report.state = InstallStateKind::InvalidInstallMetadata;
    report.profile = None;
    report.metadata = None;

    let output = InstallStatusOutput::from_report(&report);
    let rendered = render_json(&output);
    let parsed: serde_json::Value =
        serde_json::from_str(&rendered).expect("install-status JSON should parse");
    let mismatch = &parsed["data"]["mismatches"][0];

    assert_eq!(mismatch["code"], "selected_profile_unavailable");
    assert_eq!(mismatch["expected_value"], "default");
    assert_eq!(mismatch["observed_value"], "unavailable");

    let human = render_human(&output);
    assert!(human.contains("selected_profile=<unavailable>"));
    assert!(human.contains("mismatch_0_code=selected_profile_unavailable"));
    assert!(human.contains("mismatch_0_expected_value=default"));
}

#[test]
fn env_profile_override_output_is_distinct_from_stale_default() {
    let mut report = active_report();
    report.state = InstallStateKind::ExplicitOverride;
    report.config.default_profile = Some("env".to_owned());
    report.config.default_profile_source = Some(ConfigSource::Env);
    report.config.explicit_override = true;
    let profile = report.profile.as_mut().expect("active report has profile");
    profile.name = "env".to_owned();
    profile.selection_source = ConfigSource::Env;
    profile.body_source = "builtin_env";
    profile.file_path = None;
    profile.artifact_dir = None;
    profile.version_dir = None;
    profile.release_tag = None;
    profile.m80_version = None;

    let output = InstallStatusOutput::from_report(&report);
    let rendered = render_json(&output);
    let parsed: serde_json::Value =
        serde_json::from_str(&rendered).expect("install-status JSON should parse");
    let mismatch = &parsed["data"]["mismatches"][0];

    assert_eq!(mismatch["code"], "explicit_profile_override");
    assert_eq!(mismatch["expected_source"], "system_file");
    assert_eq!(mismatch["observed_source"], "env");
    assert_eq!(mismatch["expected_tag"], "v1.2.3");
    assert!(mismatch["observed_tag"].is_null());

    let human = render_human(&output);
    assert!(human.contains("status=explicit_override"));
    assert!(human.contains("mismatch_0_code=explicit_profile_override"));
    assert!(!human.contains("stale_profile_target"));
}

#[test]
fn explicit_override_diagnostics_do_not_offer_installed_state_repair_commands() {
    let mut report = active_report();
    report.state = InstallStateKind::ExplicitOverride;
    report.config.default_profile = Some("env".to_owned());
    report.config.default_profile_source = Some(ConfigSource::Env);
    report.config.explicit_override = true;
    report.diagnostics.push(InstallStateDiagnostic {
        code: InstallStateDiagnosticCode::MissingActivePointer,
        field: Some("active_pointer"),
        path: Some(PathBuf::from("/opt/m80/active")),
        message: "active install pointer is missing".to_owned(),
    });

    let output = InstallStatusOutput::from_report(&report);
    let rendered = render_json(&output);
    let parsed: serde_json::Value =
        serde_json::from_str(&rendered).expect("install-status JSON should parse");
    let diagnostic = &parsed["data"]["diagnostics"][0];

    assert_eq!(diagnostic["code"], "missing_active_pointer");
    assert!(diagnostic["repair_command"].is_null());
    assert!(diagnostic["rollback_command"].is_null());
}

#[test]
fn rollback_command_is_path_based_and_does_not_require_url_safe_tag() {
    let mut report = active_report();
    let active_dir = PathBuf::from("/opt/m80/versions/v1.2.4");
    report.state = InstallStateKind::StaleProfileTarget;
    report.active_pointer.target = Some(active_dir.clone());
    report.active_pointer.version_dir = Some(active_dir);
    report.active_pointer.release_tag = Some("v1.2.4".to_owned());
    let profile = report.profile.as_mut().expect("active report has profile");
    profile.release_tag = Some("v1.2.3+local".to_owned());
    profile.version_dir = Some(PathBuf::from("/opt/m80/versions/v1.2.3+local"));
    report.diagnostics.push(InstallStateDiagnostic {
        code: InstallStateDiagnosticCode::ProfileTargetsInactiveVersion,
        field: Some("artifact_dir"),
        path: Some(PathBuf::from("/opt/m80/versions/v1.2.3+local")),
        message: "selected profile points at a different version than active pointer".to_owned(),
    });

    let output = InstallStatusOutput::from_report(&report);
    let rendered = render_json(&output);
    let parsed: serde_json::Value =
        serde_json::from_str(&rendered).expect("install-status JSON should parse");

    assert_eq!(
        parsed["data"]["diagnostics"][0]["rollback_command"],
        "sudo ln -sfnT -- '/opt/m80/versions/v1.2.3+local' '/opt/m80/active'"
    );
}

#[test]
fn user_config_override_output_names_expected_and_observed_config_paths() {
    let mut report = active_report();
    report.state = InstallStateKind::ExplicitOverride;
    report.config.default_profile = Some("work".to_owned());
    report.config.default_profile_source = Some(ConfigSource::UserFile);
    report.config.explicit_override = true;
    report.config.user_path = Some(PathBuf::from("/home/nathan/.config/m80/config.toml"));

    let output = InstallStatusOutput::from_report(&report);
    let rendered = render_json(&output);
    let parsed: serde_json::Value =
        serde_json::from_str(&rendered).expect("install-status JSON should parse");
    let mismatch = &parsed["data"]["mismatches"][0];

    assert_eq!(mismatch["code"], "explicit_profile_override");
    assert_eq!(mismatch["expected_path"], "/etc/m80/config.toml");
    assert_eq!(
        mismatch["observed_path"],
        "/home/nathan/.config/m80/config.toml"
    );
    assert_eq!(mismatch["observed_value"], "work");

    let human = render_human(&output);
    assert!(human.contains("mismatch_0_expected_path=/etc/m80/config.toml"));
    assert!(human.contains("mismatch_0_observed_path=/home/nathan/.config/m80/config.toml"));
}

#[test]
fn install_root_override_output_names_default_and_observed_roots() {
    let mut report = active_report();
    report.install_root = PathBuf::from("/tmp/m80-install");

    let output = InstallStatusOutput::from_report(&report);
    let rendered = render_json(&output);
    let parsed: serde_json::Value =
        serde_json::from_str(&rendered).expect("install-status JSON should parse");
    let mismatch = &parsed["data"]["mismatches"][0];

    assert_eq!(mismatch["code"], "install_root_override");
    assert_eq!(mismatch["expected_path"], "/opt/m80");
    assert_eq!(mismatch["observed_path"], "/tmp/m80-install");

    let human = render_human(&output);
    assert!(human.contains("mismatch_0_code=install_root_override"));
    assert!(human.contains("mismatch_0_expected_path=/opt/m80"));
    assert!(human.contains("mismatch_0_observed_path=/tmp/m80-install"));
}

#[test]
fn status_matrix_healthy_active_release() {
    let report = active_report();

    assert_status_matrix_case(
        report,
        "healthy_active_release",
        "ready",
        "next_action=installed release is ready",
    )
    .assert_json_field("active.release_tag", "v1.2.3")
    .assert_json_field("selected_profile.name", "default")
    .assert_json_field("metadata.proof_cache_manifest.status", "present")
    .assert_json_field("proof_cache.status", "available")
    .assert_json_field("proof_cache.materials.0.role", "integrity_predicate");
}

#[test]
fn status_matrix_missing_active_pointer() {
    let mut report = active_report();
    report.state = InstallStateKind::MissingActivePointer;
    report.active_pointer.status = ActivePointerStatus::Missing;
    report.active_pointer.target = None;
    report.active_pointer.version_dir = None;
    report.active_pointer.release_tag = None;
    report.metadata = None;
    report.diagnostics.push(InstallStateDiagnostic {
        code: InstallStateDiagnosticCode::MissingActivePointer,
        field: Some("active_pointer"),
        path: Some(PathBuf::from("/opt/m80/active")),
        message: "active install pointer is missing".to_owned(),
    });

    assert_status_matrix_case(
        report,
        "missing_active_pointer",
        "install_release",
        "next_action=install a release to create the active install pointer",
    )
    .assert_json_field("active.status", "missing")
    .assert_json_null("metadata")
    .assert_json_field("proof_cache.status", "missing_active_install")
    .assert_json_field("diagnostics.0.code", "missing_active_pointer")
    .assert_json_field(
        "diagnostics.0.repair_command",
        "curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh",
    )
    .assert_json_field(
        "diagnostics.0.rollback_command",
        "sudo ln -sfnT -- '/opt/m80/versions/v1.2.3' '/opt/m80/active'",
    );
}

#[test]
fn status_matrix_dangling_active_pointer() {
    let mut report = active_report();
    let missing_dir = PathBuf::from("/opt/m80/versions/v9.9.9");
    report.state = InstallStateKind::DanglingActivePointer;
    report.active_pointer.status = ActivePointerStatus::Dangling;
    report.active_pointer.target = Some(missing_dir.clone());
    report.active_pointer.version_dir = Some(missing_dir);
    report.active_pointer.release_tag = Some("v9.9.9".to_owned());
    report.metadata = None;

    assert_status_matrix_case(
        report,
        "dangling_active_pointer",
        "reinstall_release",
        "next_action=reinstall the selected release to refresh the installed bundle",
    )
    .assert_json_field("active.status", "dangling")
    .assert_json_field("next_action.command", "curl -fsSL https://github.com/moradology/m80/releases/download/v9.9.9/install.sh | sudo sh");
}

#[test]
fn status_matrix_stale_profile_target() {
    let mut report = active_report();
    let active_dir = PathBuf::from("/opt/m80/versions/v1.2.4");
    report.state = InstallStateKind::StaleProfileTarget;
    report.active_pointer.target = Some(active_dir.clone());
    report.active_pointer.version_dir = Some(active_dir);
    report.active_pointer.release_tag = Some("v1.2.4".to_owned());
    report.metadata = None;
    report.diagnostics.push(InstallStateDiagnostic {
        code: InstallStateDiagnosticCode::ProfileTargetsInactiveVersion,
        field: Some("artifact_dir"),
        path: Some(PathBuf::from("/opt/m80/versions/v1.2.3")),
        message: "selected profile points at a different version than active pointer".to_owned(),
    });

    assert_status_matrix_case(
        report,
        "stale_profile_target",
        "reinstall_release",
        "next_action=reinstall the selected release to refresh the installed bundle",
    )
    .assert_json_field("mismatches.0.code", "stale_profile_target")
    .assert_json_field("mismatches.0.expected_tag", "v1.2.4")
    .assert_json_field("mismatches.0.observed_tag", "v1.2.3")
    .assert_json_field(
        "diagnostics.0.repair_command",
        "curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.4/install.sh | sudo sh",
    )
    .assert_json_field(
        "diagnostics.0.rollback_command",
        "sudo ln -sfnT -- '/opt/m80/versions/v1.2.3' '/opt/m80/active'",
    );
}

#[test]
fn status_matrix_explicit_override() {
    let mut report = active_report();
    report.state = InstallStateKind::ExplicitOverride;
    report.config.default_profile = Some("env".to_owned());
    report.config.default_profile_source = Some(ConfigSource::Env);
    report.config.explicit_override = true;
    let profile = report.profile.as_mut().expect("active report has profile");
    profile.name = "env".to_owned();
    profile.selection_source = ConfigSource::Env;
    profile.body_source = "builtin_env";
    profile.file_path = None;
    profile.artifact_dir = None;
    profile.version_dir = None;
    profile.release_tag = None;
    profile.m80_version = None;
    report.metadata = None;

    assert_status_matrix_case(
        report,
        "explicit_override",
        "remove_override",
        "next_action=remove the profile override to inspect the installed default release",
    )
    .assert_json_field("selected_config.default_profile_source", "env")
    .assert_json_field("mismatches.0.code", "explicit_profile_override")
    .assert_json_absent("next_action.command");
}

#[test]
fn status_matrix_explicit_override_flag_source() {
    let mut report = active_report();
    report.state = InstallStateKind::ExplicitOverride;
    report.config.default_profile = Some("other".to_owned());
    report.config.default_profile_source = Some(ConfigSource::Flag);
    report.config.explicit_override = true;
    let profile = report.profile.as_mut().expect("active report has profile");
    profile.name = "other".to_owned();
    profile.selection_source = ConfigSource::Flag;
    profile.release_tag = Some("v9.0.0".to_owned());

    assert_status_matrix_case(
        report,
        "explicit_override",
        "remove_override",
        "next_action=remove the profile override to inspect the installed default release",
    )
    .assert_json_field("selected_config.default_profile_source", "flag")
    .assert_json_field("selected_profile.name", "other")
    .assert_json_field("mismatches.0.code", "explicit_profile_override")
    .assert_json_absent("mismatches.0.observed_path");
}

#[test]
fn status_matrix_local_dev_tree() {
    let mut report = active_report();
    report.state = InstallStateKind::LocalDevTree;
    report.active_pointer.status = ActivePointerStatus::Missing;
    report.active_pointer.target = None;
    report.active_pointer.version_dir = None;
    report.active_pointer.release_tag = None;
    report.config.default_profile = Some("env".to_owned());
    report.config.default_profile_source = Some(ConfigSource::Default);
    let profile = report.profile.as_mut().expect("active report has profile");
    profile.name = "env".to_owned();
    profile.selection_source = ConfigSource::Default;
    profile.body_source = "builtin_env";
    profile.file_path = None;
    profile.artifact_dir = None;
    profile.version_dir = None;
    profile.release_tag = None;
    profile.m80_version = None;
    report.metadata = None;

    assert_status_matrix_case(
        report,
        "local_dev_tree",
        "install_release",
        "next_action=install a release so m80 run uses the bundled guest by default",
    )
    .assert_json_field("selected_profile.body_source", "builtin_env")
    .assert_json_field("selected_config.default_profile", "env")
    .assert_json_field("proof_cache.status", "local_dev_install");
}

#[test]
fn status_matrix_tampered_proof_cache() {
    let mut report = active_report();
    report.state = InstallStateKind::TamperedProofCache;
    let metadata = report
        .metadata
        .as_mut()
        .expect("active report has metadata");
    metadata.proof_cache_manifest.status = MetadataFileStatus::Stale;
    metadata.proof_cache = None;
    report.diagnostics.push(InstallStateDiagnostic {
        code: InstallStateDiagnosticCode::ProofCacheStale,
        field: Some("proof_cache_manifest"),
        path: Some(PathBuf::from(
            "/opt/m80/versions/v1.2.3/artifacts/release-proof-cache/manifest.json",
        )),
        message: "proof-cache mode mismatch: expected=644 observed=600".to_owned(),
    });

    let human = render_human(&InstallStatusOutput::from_report(&report));
    assert!(human.contains("proof_cache_diagnostic_0_code=proof_cache_stale"));
    assert!(human.contains("proof_cache_repair_command=curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.3/install.sh | sudo sh"));

    assert_status_matrix_case(
        report,
        "tampered_proof_cache",
        "reinstall_release",
        "next_action=reinstall the selected release to refresh the installed bundle",
    )
    .assert_json_field("metadata.proof_cache_manifest.status", "stale")
    .assert_json_field("proof_cache.status", "stale_manifest")
    .assert_json_field("proof_cache.diagnostics.0.code", "proof_cache_stale")
    .assert_json_field(
        "proof_cache.repair_command",
        "curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.3/install.sh | sudo sh",
    )
    .assert_json_field("next_action.command", "curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.3/install.sh | sudo sh");
}

#[test]
fn human_output_for_missing_install_has_next_action() {
    let mut report = active_report();
    report.state = InstallStateKind::MissingActivePointer;
    report.active_pointer.status = ActivePointerStatus::Missing;
    report.active_pointer.release_tag = None;
    report.active_pointer.version_dir = None;
    report.metadata = None;
    report.diagnostics.push(InstallStateDiagnostic {
        code: InstallStateDiagnosticCode::MissingActivePointer,
        field: Some("active_pointer"),
        path: Some(PathBuf::from("/opt/m80/active")),
        message: "active install pointer is missing".to_owned(),
    });

    let rendered = render_human(&InstallStatusOutput::from_report(&report));

    assert!(rendered.contains("status=missing_active_pointer"));
    assert!(rendered.contains("diagnostic_0_code=missing_active_pointer"));
    assert!(rendered.contains("diagnostic_0_path=/opt/m80/active"));
    assert!(rendered.contains("diagnostic_0_repair_command=curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh"));
    assert!(rendered.contains("diagnostic_0_rollback_command=sudo ln -sfnT -- '/opt/m80/versions/v1.2.3' '/opt/m80/active'"));
    assert!(rendered.contains("bundle_metadata_status=unavailable"));
    assert!(rendered.contains("proof_cache_manifest_status=unavailable"));
    assert!(rendered.contains("proof_cache_status=missing_active_install"));
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
            proof_cache: Some(proof_cache_report(&artifacts_dir)),
        }),
        diagnostics: Vec::new(),
    }
}

fn proof_cache_report(artifacts_dir: &std::path::Path) -> ProofCacheReport {
    let cache_dir = artifacts_dir.join("release-proof-cache");
    ProofCacheReport {
        cache_dir: cache_dir.clone(),
        manifest_path: cache_dir.join("manifest.json"),
        release_tag: "v1.2.3".to_owned(),
        repository: "moradology/m80".to_owned(),
        target: "linux-x86_64".to_owned(),
        manifest_digest: "1".repeat(64),
        manifest_modified_unix_seconds: Some(1_800_000_000),
        cache_age_seconds: Some(30),
        materials: vec![ProofCacheMaterialReport {
            role: "integrity_predicate".to_owned(),
            path: "m80-release-integrity.json".to_owned(),
            sha256: "2".repeat(64),
            size_bytes: Some(1200),
            subject: None,
            modified_unix_seconds: Some(1_800_000_000),
        }],
        trust_policy: ProofCacheTrustPolicyReport {
            path: "m80-release-trust-policy.json".to_owned(),
            identity: "repository=moradology/m80".to_owned(),
            sha256: "3".repeat(64),
            modified_unix_seconds: Some(1_800_000_000),
        },
        verifier_versions: ProofCacheVerifierVersionsReport {
            m80_version: "v1.2.3".to_owned(),
            attestation_verifier: "m80 native release-attestation verifier v1".to_owned(),
            release_integrity_schema_version: 1,
            asset_index_schema_version: 1,
        },
    }
}

fn metadata_file(path: PathBuf) -> MetadataFileReport {
    MetadataFileReport {
        path,
        status: MetadataFileStatus::Present,
        sha256: Some("0".repeat(64)),
    }
}

struct StatusMatrixAssertion {
    data: serde_json::Value,
}

impl StatusMatrixAssertion {
    fn assert_json_field(self, dotted_path: &str, expected: &str) -> Self {
        assert_eq!(
            json_field(&self.data, dotted_path).as_str(),
            Some(expected),
            "{dotted_path}"
        );
        self
    }

    fn assert_json_null(self, dotted_path: &str) -> Self {
        assert!(
            json_field(&self.data, dotted_path).is_null(),
            "{dotted_path} should be null"
        );
        self
    }

    fn assert_json_absent(self, dotted_path: &str) -> Self {
        assert!(
            json_field(&self.data, dotted_path).is_null(),
            "{dotted_path} should be absent or null"
        );
        self
    }
}

fn assert_status_matrix_case(
    report: InstallStateReport,
    expected_status: &str,
    expected_next_action_kind: &str,
    expected_human_next_action: &str,
) -> StatusMatrixAssertion {
    let output = InstallStatusOutput::from_report(&report);
    let rendered = render_json(&output);
    let parsed: serde_json::Value =
        serde_json::from_str(&rendered).expect("install-status JSON should parse");
    let data = parsed["data"].clone();

    assert_eq!(data["schema_version"], 1);
    assert_eq!(data["status"], expected_status);
    assert_eq!(data["next_action"]["kind"], expected_next_action_kind);
    assert!(
        data.get("active").is_some(),
        "active field should be present"
    );
    assert!(
        data.get("selected_config").is_some(),
        "selected_config field should be present"
    );

    let human = render_human(&output);
    assert!(
        human.contains(&format!("status={expected_status}")),
        "human status missing {expected_status}: {human}"
    );
    assert!(
        human.contains(expected_human_next_action),
        "human next_action missing {expected_human_next_action:?}: {human}"
    );

    StatusMatrixAssertion { data }
}

fn json_field<'a>(value: &'a serde_json::Value, dotted_path: &str) -> &'a serde_json::Value {
    let mut current = value;
    for segment in dotted_path.split('.') {
        if let Ok(index) = segment.parse::<usize>() {
            current = &current[index];
        } else {
            current = &current[segment];
        }
    }
    current
}
