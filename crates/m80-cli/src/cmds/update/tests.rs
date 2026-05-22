use std::{fs, path::PathBuf};

use m80_firecracker::{ConfigError, ConfigFilePaths, ConfigSource, FcError};

use super::latest_status::parse_latest_status;
use super::*;
use crate::install_state::{
    ActivePointerReport, ActivePointerStatus, InstallConfigReport, InstallMetadataReport,
    InstallProfileReport, MetadataFileReport, MetadataFileStatus, ProofCacheMaterialReport,
    ProofCacheReport, ProofCacheTrustPolicyReport, ProofCacheVerifierVersionsReport,
};
use crate::release_freshness::read_freshness_status_artifact_json;

mod active_kind;
mod http_fixture;
mod no_write;
mod offline_cache;
mod proof_cache_fixture;
mod repair_commands;

#[test]
fn update_check_reports_current_release_with_cached_proof_age() {
    let report = active_report("v1.2.3");
    let output = check_output(
        &report,
        latest("v1.2.3", None, &[]),
        UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
    );

    assert_eq!(output.state, UpdateCheckState::Current);
    assert_eq!(output.active_tag.as_deref(), Some("v1.2.3"));
    assert_eq!(output.latest_stable_tag.as_deref(), Some("v1.2.3"));
    assert_eq!(output.safety_state, SafetyFloorStatus::Unknown);
    assert_eq!(output.proof_cache_status, UpdateProofCacheStatus::Available);
    assert_eq!(output.proof_cache_age_seconds, Some(30));
    assert_eq!(
        output.next_command.as_deref(),
        Some("m80 run -- echo hello")
    );
}

#[test]
fn update_check_reports_outdated_release_with_exact_apply_command() {
    let report = active_report("v1.2.3");
    let output = check_output(
        &report,
        latest("v1.2.4", Some("v1.2.0"), &[]),
        UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
    );

    assert_eq!(output.state, UpdateCheckState::Outdated);
    assert_eq!(output.safety_state, SafetyFloorStatus::Safe);
    assert_eq!(output.safety_floor.status, SafetyFloorStatus::Safe);
    assert_eq!(
        output.apply_command.as_deref(),
        Some("curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.4/install.sh | sudo sh")
    );
    assert_eq!(output.next_command, output.apply_command);
}

#[test]
fn update_check_reports_stale_proof_cache_before_available_metadata() {
    let mut report = active_report("v1.2.3");
    report.state = InstallStateKind::TamperedProofCache;
    report.diagnostics.push(InstallStateDiagnostic {
        code: InstallStateDiagnosticCode::ProofCacheStale,
        field: Some("proof_cache.release_tag"),
        path: Some(PathBuf::from(
            "/opt/m80/versions/v1.2.3/artifacts/release-proof-cache/manifest.json",
        )),
        message: "proof cache release_tag does not match bundle metadata".to_owned(),
    });

    let output = check_output(
        &report,
        latest("v1.2.3", None, &[]),
        UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
    );

    assert_eq!(
        output.proof_cache_status,
        UpdateProofCacheStatus::StaleManifest
    );
}

#[test]
fn update_check_reports_yanked_active_release() {
    let report = active_report("v1.2.3");
    let output = check_output(
        &report,
        latest("v1.2.4", Some("v1.2.0"), &["v1.2.3"]),
        UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
    );

    assert_eq!(output.state, UpdateCheckState::Yanked);
    assert_eq!(output.safety_state, SafetyFloorStatus::ActiveYanked);
    assert_eq!(output.safety_floor.status, SafetyFloorStatus::ActiveYanked);
    assert_eq!(
        output.apply_command.as_deref(),
        Some("curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.0/install.sh | sudo sh")
    );
    assert_eq!(output.safety_floor.policy_tag.as_deref(), Some("v1.2.3"));
    assert_eq!(output.safety_floor.reason.as_deref(), Some("bad release"));
    assert_eq!(
        output.safety_floor.advisory_url.as_deref(),
        Some("https://github.com/moradology/m80/issues/1")
    );
    assert_eq!(
        output.safety_floor.published_at.as_deref(),
        Some("2026-05-21T12:00:00Z")
    );
    assert_eq!(
        output.safety_floor.replacement_command.as_deref(),
        Some("curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.0/install.sh | sudo sh")
    );
    assert_eq!(
        output.safety_floor.metadata_source.as_deref(),
        Some("fixture")
    );
    let human = render_human(&output);
    assert!(human.contains("safety_state=active_yanked\n"));
    assert!(human.contains("safety_floor_policy_tag=v1.2.3\n"));
    assert!(human.contains("safety_floor_reason=bad release\n"));
    assert!(
        human.contains("safety_floor_advisory_url=https://github.com/moradology/m80/issues/1\n")
    );
    assert!(human.contains(
        "safety_floor_replacement_command=curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.0/install.sh | sudo sh\n"
    ));
    let json: serde_json::Value =
        serde_json::from_str(&crate::json::to_pretty(&output)).expect("update JSON parses");
    assert_eq!(json["data"]["safety_state"], "active_yanked");
    assert_eq!(json["data"]["safety_floor"]["policy_tag"], "v1.2.3");
    assert_eq!(json["data"]["safety_floor"]["reason"], "bad release");
    assert_eq!(
        json["data"]["safety_floor"]["advisory_url"],
        "https://github.com/moradology/m80/issues/1"
    );
    assert_eq!(
        json["data"]["safety_floor"]["replacement_command"],
        "curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.0/install.sh | sudo sh"
    );
    assert_eq!(json["data"]["safety_floor"]["metadata_source"], "fixture");
}

#[test]
fn update_check_uses_metadata_repair_command_when_latest_target_is_yanked() {
    let report = active_report("v1.2.3");
    let output = check_output(
        &report,
        latest("v1.2.4", Some("v1.2.0"), &["v1.2.4"]),
        UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
    );

    assert_eq!(output.state, UpdateCheckState::Yanked);
    assert_eq!(output.safety_state, SafetyFloorStatus::LatestYanked);
    assert_eq!(output.safety_floor.status, SafetyFloorStatus::LatestYanked);
    assert_eq!(
        output.apply_command.as_deref(),
        Some("curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.0/install.sh | sudo sh")
    );
    assert_eq!(output.next_command, output.apply_command);
}

#[test]
fn update_check_reports_unsafe_active_release_below_floor() {
    let report = active_report("v1.1.9");
    let output = check_output(
        &report,
        latest("v1.2.4", Some("v1.2.0"), &[]),
        UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
    );

    assert_eq!(output.state, UpdateCheckState::Unsafe);
    assert_eq!(output.safety_state, SafetyFloorStatus::ActiveBelowMinimum);
    assert_eq!(
        output.safety_floor.status,
        SafetyFloorStatus::ActiveBelowMinimum
    );
    assert_eq!(output.safety_floor.policy_tag.as_deref(), Some("v1.2.0"));
    assert_eq!(
        output.safety_floor.reason.as_deref(),
        Some("security floor")
    );
    assert_eq!(
        output.safety_floor.issue_id.as_deref(),
        Some("m80-o3uh9.21.9")
    );
    assert_eq!(
        output.safety_floor.replacement_command.as_deref(),
        Some("curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.0/install.sh | sudo sh")
    );
    assert_eq!(
        output.apply_command.as_deref(),
        Some("curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.0/install.sh | sudo sh")
    );
}

#[test]
fn update_check_rejects_latest_target_below_floor_status() {
    let err = parse_latest_status(
        "fixture".to_owned(),
        status_artifact("v1.1.9", Some("v1.2.0"), &[]),
    );

    let err = err.expect_err("latest status below safety floor should fail before update output");
    match err {
        FcError::Config(ConfigError::InvalidValue { field, reason }) => {
            assert_eq!(field, "update.latest_status");
            assert!(reason.contains("safety_floor.minimum_safe_tag.tag"));
            assert!(reason.contains("v1.2.0"));
            assert!(reason.contains("v1.1.9"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn update_check_reports_stale_safety_artifact_before_policy_state() {
    let metadata = read_freshness_status_artifact_json(&status_artifact_at(
        "v1.2.4",
        Some("v1.2.0"),
        &["v1.1.9"],
        "2026-05-19T11:59:59Z",
    ))
    .expect("stale status fixture should parse");
    let output = check_output(
        &active_report("v1.1.9"),
        LatestStatusInput::Available {
            source: "fixture-stale".to_owned(),
            metadata,
            origin: LatestStatusOrigin::CacheFallback,
            offline_reason: Some("remote unavailable".to_owned()),
        },
        UnixSeconds::new(timestamp("2026-05-21T12:00:00Z")),
    );

    assert_eq!(output.state, UpdateCheckState::StaleLatestMetadata);
    assert_eq!(output.safety_state, SafetyFloorStatus::StaleMetadata);
    assert_eq!(output.safety_floor.status, SafetyFloorStatus::StaleMetadata);
    assert_eq!(output.apply_command, None);
    assert_eq!(output.retry_command.as_deref(), Some("m80 update --check"));
    let human = render_human(&output);
    assert!(human.contains("safety_state=stale_metadata\n"));
    assert!(human.contains("safety_floor_status=stale_metadata\n"));
}

#[test]
fn malformed_safety_artifact_fails_closed_before_update_state() {
    let malformed = status_artifact("v1.2.4", None, &[]).replace(
        r#""minimum_safe_tag":null"#,
        r#""minimum_safe_tag":{"tag":"latest","reason":"bad floor","advisory_url":"https://github.com/moradology/m80/security/advisories/GHSA-test","issue_id":null,"replacement_command":"curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.4/install.sh | sudo sh"}"#,
    );

    let err = parse_latest_status("fixture".to_owned(), malformed)
        .expect_err("malformed safety floor should fail before update output");

    match err {
        FcError::Config(ConfigError::InvalidValue { field, reason }) => {
            assert_eq!(field, "update.latest_status");
            assert!(reason.contains("safety_floor.minimum_safe_tag.tag"));
            assert!(reason.contains("latest"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn update_check_reports_unknown_offline_without_mutation_command() {
    let report = active_report("v1.2.3");
    let output = check_output(
        &report,
        LatestStatusInput::UnknownOffline {
            source: "https://example.invalid/status.json".to_owned(),
            detail: "network unavailable".to_owned(),
            origin: LatestStatusOrigin::Unavailable,
            cache_state: LatestStatusCacheState::NotConfigured,
        },
        UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
    );

    assert_eq!(output.state, UpdateCheckState::UnknownOffline);
    assert_eq!(output.latest_stable_tag, None);
    assert_eq!(
        output.latest_status_error.as_deref(),
        Some("network unavailable")
    );
    assert_eq!(output.apply_command, None);
    assert_eq!(output.reinstall_command, None);
    assert_eq!(output.retry_command.as_deref(), Some("m80 update --check"));
    assert_eq!(output.next_command, output.retry_command);
    let human = render_human(&output);
    assert!(human.contains("retry_command=m80 update --check\n"));
    assert!(human.contains("message=latest status is unavailable; active freshness is unknown\n"));
}

#[test]
fn update_check_does_not_write_install_root_when_checking_unhealthy_state() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install");
    fs::create_dir(&install_root).unwrap();
    let sentinel = install_root.join("sentinel");
    fs::write(&sentinel, "keep").unwrap();
    let before = install_root_entries(&install_root);

    let args = UpdateArgs {
        check: true,
        install_root: install_root.clone(),
        profile: None,
        latest_status: None,
        latest_status_url: Some("http://127.0.0.1:9/latest.json".to_owned()),
        config_path: Some(temp.path().join("missing-config.toml")),
        profile_dir: Some(temp.path().join("profiles")),
    };

    let status = cmd_update(args, false).unwrap();

    assert_eq!(status, 0);
    assert_eq!(fs::read_to_string(&sentinel).unwrap(), "keep");
    assert_eq!(install_root_entries(&install_root), before);
}

#[test]
fn update_check_reports_local_dev_without_latest_guessing() {
    let mut report = active_report("v1.2.3");
    report.state = InstallStateKind::LocalDevTree;
    report.active_pointer.release_tag = None;
    report.metadata = None;

    let output = check_output(
        &report,
        latest("v1.2.4", Some("v1.2.0"), &[]),
        UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
    );

    assert_eq!(output.state, UpdateCheckState::LocalDevInstall);
    assert_eq!(output.apply_command, None);
    assert_eq!(
        output.proof_cache_status,
        UpdateProofCacheStatus::LocalDevInstall
    );
}

#[test]
fn update_check_fetches_latest_only_for_healthy_release_state() {
    let mut report = active_report("v1.2.3");
    assert!(needs_latest_status(&report));

    report.state = InstallStateKind::LocalDevTree;
    assert!(!needs_latest_status(&report));

    report.state = InstallStateKind::TamperedProofCache;
    assert!(!needs_latest_status(&report));
}

#[test]
fn malformed_latest_status_fails_closed() {
    let err = parse_latest_status(
        "fixture".to_owned(),
        status_artifact_with_resolved_tag(None, None, &[]),
    )
    .expect_err("malformed status should fail");

    match err {
        FcError::Config(ConfigError::InvalidValue { field, reason }) => {
            assert_eq!(field, "update.latest_status");
            assert!(
                reason.contains("freshness status missing resolved_tag"),
                "{reason}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn update_check_paths_use_fixture_overrides_without_host_paths() {
    let args = UpdateArgs {
        check: true,
        install_root: PathBuf::from("/tmp/m80"),
        profile: None,
        latest_status: None,
        latest_status_url: None,
        config_path: Some(PathBuf::from("/tmp/config.toml")),
        profile_dir: Some(PathBuf::from("/tmp/profiles")),
    };

    let paths = install_state_paths(&args);

    assert_eq!(paths.install_root, PathBuf::from("/tmp/m80"));
    assert_eq!(
        paths.config_paths,
        ConfigFilePaths {
            system: Some(PathBuf::from("/tmp/config.toml")),
            system_drop_in_dir: None,
            user: None,
            user_drop_in_dir: None,
        }
    );
    assert_eq!(
        paths.profile_paths.system_dir.as_deref(),
        Some(std::path::Path::new("/tmp/profiles"))
    );
}

fn latest(tag: &str, minimum_safe_tag: Option<&str>, yanked_tags: &[&str]) -> LatestStatusInput {
    LatestStatusInput::Available {
        source: "fixture".to_owned(),
        metadata: read_freshness_status_artifact_json(&status_artifact(
            tag,
            minimum_safe_tag,
            yanked_tags,
        ))
        .expect("freshness fixture should parse"),
        origin: LatestStatusOrigin::Remote,
        offline_reason: None,
    }
}

fn status_artifact(tag: &str, minimum_safe_tag: Option<&str>, yanked_tags: &[&str]) -> String {
    status_artifact_with_resolved_tag(Some(tag), minimum_safe_tag, yanked_tags)
}

fn status_artifact_with_resolved_tag(
    tag: Option<&str>,
    minimum_safe_tag: Option<&str>,
    yanked_tags: &[&str],
) -> String {
    status_artifact_at_option(tag, minimum_safe_tag, yanked_tags, "2026-05-21T12:00:00Z")
}

fn status_artifact_at(
    tag: &str,
    minimum_safe_tag: Option<&str>,
    yanked_tags: &[&str],
    published_at: &str,
) -> String {
    status_artifact_at_option(Some(tag), minimum_safe_tag, yanked_tags, published_at)
}

fn status_artifact_at_option(
    tag: Option<&str>,
    minimum_safe_tag: Option<&str>,
    yanked_tags: &[&str],
    published_at: &str,
) -> String {
    let resolved_tag = tag
        .map(|tag| format!(r#""resolved_tag":"{tag}","#))
        .unwrap_or_default();
    let asset_tag = tag.unwrap_or("v1.2.3");
    let replacement_tag = minimum_safe_tag.unwrap_or(asset_tag);
    let safety_floor =
        safety_floor_json_at(minimum_safe_tag, yanked_tags, replacement_tag, published_at);
    format!(
        r#"{{
          "schema_version":1,
          "freshness_network_bounded":true,
          "repository":"moradology/m80",
          {resolved_tag}
          "published_at":"{published_at}",
          "fetch_policy":{{"connect_timeout_seconds":10,"max_time_seconds":120,"retry_count":2,"retry_delay_seconds":1}},
          "checked_urls":[{{"role":"latest-install","url":"{}","release_tag":"latest","asset_name":"install.sh","sources":["release-url-contract:latest-install"],"size_bytes":123,"sha256":"{}"}}],
          "public_assets":[{{"name":"install.sh","role":"installer","url":"{}","release_tag":"{asset_tag}","size_bytes":123,"sha256":"{}"}}],
          "safety_floor":{safety_floor}
        }}"#,
        crate::release_urls::latest_install_url(),
        "1".repeat(64),
        crate::release_urls::release_install_url(asset_tag),
        "2".repeat(64),
    )
}

fn safety_floor_json(
    minimum_safe_tag: Option<&str>,
    yanked_tags: &[&str],
    replacement_tag: &str,
) -> String {
    safety_floor_json_at(
        minimum_safe_tag,
        yanked_tags,
        replacement_tag,
        "2026-05-21T12:00:00Z",
    )
}

fn safety_floor_json_at(
    minimum_safe_tag: Option<&str>,
    yanked_tags: &[&str],
    replacement_tag: &str,
    published_at: &str,
) -> String {
    let minimum = minimum_safe_tag
        .map(|tag| {
            format!(
                r#"{{"tag":"{tag}","reason":"security floor","advisory_url":null,"issue_id":"m80-o3uh9.21.9","replacement_command":"{}"}}"#,
                pinned_install_command(replacement_tag)
            )
        })
        .unwrap_or_else(|| "null".to_owned());
    let yanked = yanked_tags
        .iter()
        .map(|tag| {
            format!(
                r#"{{"tag":"{tag}","reason":"bad release","advisory_url":"https://github.com/moradology/m80/issues/1","issue_id":null,"published_at":"{published_at}","replacement_command":"{}","no_replacement_reason":null}}"#,
                pinned_install_command(replacement_tag)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"schema_version":1,"published_at":"{published_at}","minimum_safe_tag":{minimum},"yanked_releases":[{yanked}]}}"#
    )
}

fn active_report(tag: &str) -> InstallStateReport {
    let version_dir = PathBuf::from("/opt/m80/versions").join(tag);
    let artifacts_dir = version_dir.join("artifacts");
    InstallStateReport {
        state: InstallStateKind::HealthyActiveRelease,
        install_root: PathBuf::from("/opt/m80"),
        active_pointer: ActivePointerReport {
            path: PathBuf::from("/opt/m80/active"),
            target: Some(version_dir.clone()),
            version_dir: Some(version_dir.clone()),
            release_tag: Some(tag.to_owned()),
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
            release_tag: Some(tag.to_owned()),
            m80_version: Some(tag.trim_start_matches('v').to_owned()),
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
            proof_cache: Some(proof_cache_report(&artifacts_dir, tag)),
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

fn proof_cache_report(artifacts_dir: &std::path::Path, tag: &str) -> ProofCacheReport {
    let cache_dir = artifacts_dir.join("release-proof-cache");
    ProofCacheReport {
        cache_dir: cache_dir.clone(),
        manifest_path: cache_dir.join("manifest.json"),
        release_tag: tag.to_owned(),
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
            m80_version: tag.to_owned(),
            gh_version: "gh version 2.0.0".to_owned(),
            release_integrity_schema_version: 1,
            asset_index_schema_version: 1,
        },
    }
}

fn install_root_entries(path: &std::path::Path) -> Vec<String> {
    let mut entries = fs::read_dir(path)
        .unwrap()
        .map(|entry| {
            entry
                .unwrap()
                .file_name()
                .into_string()
                .expect("test file names are utf-8")
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

fn timestamp(input: &str) -> i64 {
    crate::release_freshness::read_freshness_status_artifact_json(&format!(
        r#"{{
          "schema_version":1,
          "freshness_network_bounded":true,
          "repository":"moradology/m80",
          "resolved_tag":"v1.2.3",
          "published_at":"{input}",
          "fetch_policy":{{"connect_timeout_seconds":10,"max_time_seconds":120,"retry_count":2,"retry_delay_seconds":1}},
          "checked_urls":[{{"role":"latest-install","url":"{}","release_tag":"latest","asset_name":"install.sh","sources":["release-url-contract:latest-install"],"size_bytes":123,"sha256":"{}"}}],
          "public_assets":[{{"name":"install.sh","role":"installer","url":"{}","release_tag":"v1.2.3","size_bytes":123,"sha256":"{}"}}],
          "safety_floor":{}
        }}"#,
        crate::release_urls::latest_install_url(),
        "1".repeat(64),
        crate::release_urls::release_install_url("v1.2.3"),
        "2".repeat(64),
        safety_floor_json(None, &[], "v1.2.3"),
    ))
    .expect("timestamp fixture parses")
    .published_at()
    .as_i64()
}
