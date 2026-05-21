use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::os::unix::fs::symlink;
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::*;

fn args_with_release_tag(tag: &str) -> InstallArgs {
    InstallArgs {
        release_tag: Some(tag.to_owned()),
        bundle_url: None,
        bootstrap_tag: None,
        install_root: PathBuf::from("/tmp/m80-install"),
        dry_run: true,
    }
}

fn args_with_bundle_url(url: &str) -> InstallArgs {
    InstallArgs {
        release_tag: None,
        bundle_url: Some(url.to_owned()),
        bootstrap_tag: None,
        install_root: PathBuf::from("/tmp/m80-install"),
        dry_run: true,
    }
}

#[test]
fn release_tag_source_selects_index_bundle_for_tagged_binary() {
    let identity = VersionIdentity::from_parts(
        "1.2.3",
        Some("v1.2.3"),
        Some("0123456789abcdef0123456789abcdef01234567"),
    );
    let temp = tempfile::tempdir().unwrap();
    let index_url = write_index_with_sidecar(
        temp.path(),
        &index_json_with_assets(asset_json("linux", "x86_64", "minimal", "v1.2.3")),
    );
    let plan =
        source_plan_from_test_index(InstallSource::ReleaseTag("v1.2.3"), &identity, &index_url)
            .unwrap();

    assert_eq!(plan.kind, SourceKind::PinnedVersion);
    assert_eq!(plan.release_tag.as_deref(), Some("v1.2.3"));
    assert_eq!(
        plan.bundle_url.as_deref(),
        Some("https://github.com/moradology/m80/releases/download/v1.2.3/m80-linux-x86_64.tar.gz")
    );
}

#[test]
fn bootstrap_tag_source_selects_index_bundle_for_tagged_binary() {
    let identity = VersionIdentity::from_parts(
        "1.2.3",
        Some("v1.2.3"),
        Some("0123456789abcdef0123456789abcdef01234567"),
    );
    let temp = tempfile::tempdir().unwrap();
    let index_url = write_index_with_sidecar(
        temp.path(),
        &index_json_with_assets(asset_json("linux", "x86_64", "minimal", "v1.2.3")),
    );
    let plan =
        source_plan_from_test_index(InstallSource::BootstrapTag("v1.2.3"), &identity, &index_url)
            .unwrap();

    assert_eq!(plan.kind, SourceKind::BootstrapTag);
    assert_eq!(plan.release_tag.as_deref(), Some("v1.2.3"));
    assert_eq!(
        plan.bundle_url.as_deref(),
        Some("https://github.com/moradology/m80/releases/download/v1.2.3/m80-linux-x86_64.tar.gz")
    );
}

#[test]
fn release_tag_source_rejects_missing_default_index_entry() {
    let identity = VersionIdentity::from_parts(
        "1.2.3",
        Some("v1.2.3"),
        Some("0123456789abcdef0123456789abcdef01234567"),
    );
    let temp = tempfile::tempdir().unwrap();
    let index_url = write_index_with_sidecar(temp.path(), &index_json_with_assets(String::new()));

    let err =
        source_plan_from_test_index(InstallSource::ReleaseTag("v1.2.3"), &identity, &index_url)
            .unwrap_err();

    assert!(err.to_string().contains("no default bundle"), "{err}");
    let diagnostic = asset_index_diagnostic(&err);
    assert_eq!(
        diagnostic.code,
        release_asset_index::AssetIndexDiagnosticCode::MissingDefaultBundle
    );
    assert_eq!(diagnostic.requested_release_tag, "v1.2.3");
    assert_eq!(diagnostic.requested_m80_version, "v1.2.3");
}

#[test]
fn release_tag_source_rejects_duplicate_default_index_entry() {
    let identity = VersionIdentity::from_parts(
        "1.2.3",
        Some("v1.2.3"),
        Some("0123456789abcdef0123456789abcdef01234567"),
    );
    let temp = tempfile::tempdir().unwrap();
    let index_url = write_index_with_sidecar(
        temp.path(),
        &index_json_with_assets(format!(
            "{},{}",
            asset_json("linux", "x86_64", "minimal", "v1.2.3"),
            asset_json("linux", "x86_64", "minimal", "v1.2.3")
        )),
    );

    let err =
        source_plan_from_test_index(InstallSource::ReleaseTag("v1.2.3"), &identity, &index_url)
            .unwrap_err();

    assert!(
        err.to_string().contains("duplicate default bundles"),
        "{err}"
    );
    assert_eq!(
        asset_index_diagnostic(&err).code,
        release_asset_index::AssetIndexDiagnosticCode::DuplicateDefaultBundle
    );
}

#[test]
fn release_tag_source_rejects_wrong_arch_index_entry() {
    let identity = VersionIdentity::from_parts(
        "1.2.3",
        Some("v1.2.3"),
        Some("0123456789abcdef0123456789abcdef01234567"),
    );
    let temp = tempfile::tempdir().unwrap();
    let index_url = write_index_with_sidecar(
        temp.path(),
        &index_json_with_assets(asset_json("linux", "aarch64", "minimal", "v1.2.3")),
    );

    let err =
        source_plan_from_test_index(InstallSource::ReleaseTag("v1.2.3"), &identity, &index_url)
            .unwrap_err();

    assert!(err.to_string().contains("os=linux arch=x86_64"), "{err}");
    let diagnostic = asset_index_diagnostic(&err);
    assert_eq!(
        diagnostic.code,
        release_asset_index::AssetIndexDiagnosticCode::UnsupportedHostTuple
    );
    assert!(
        diagnostic
            .available_tuples
            .contains(&"linux/aarch64/minimal@v1.2.3".to_owned()),
        "{diagnostic:?}"
    );
}

#[test]
fn release_tag_source_rejects_wrong_image_kind_index_entry() {
    let identity = VersionIdentity::from_parts(
        "1.2.3",
        Some("v1.2.3"),
        Some("0123456789abcdef0123456789abcdef01234567"),
    );
    let temp = tempfile::tempdir().unwrap();
    let index_url = write_index_with_sidecar(
        temp.path(),
        &index_json_with_assets(asset_json("linux", "x86_64", "ubuntu", "v1.2.3")),
    );

    let err =
        source_plan_from_test_index(InstallSource::ReleaseTag("v1.2.3"), &identity, &index_url)
            .unwrap_err();

    assert!(
        err.to_string().contains("requested image kind minimal"),
        "{err}"
    );
    let diagnostic = asset_index_diagnostic(&err);
    assert_eq!(
        diagnostic.code,
        release_asset_index::AssetIndexDiagnosticCode::MissingImageKind
    );
    assert_eq!(diagnostic.available_image_kinds, vec!["ubuntu"]);
}

#[test]
fn release_tag_source_rejects_stale_index_version_with_code() {
    let identity = VersionIdentity::from_parts(
        "1.2.3",
        Some("v1.2.3"),
        Some("0123456789abcdef0123456789abcdef01234567"),
    );
    let temp = tempfile::tempdir().unwrap();
    let index_url = write_index_with_sidecar(
        temp.path(),
        &index_json_with_assets(asset_json_with_m80_version(
            "linux", "x86_64", "minimal", "v1.2.3", "v9.9.9",
        )),
    );

    let err =
        source_plan_from_test_index(InstallSource::ReleaseTag("v1.2.3"), &identity, &index_url)
            .unwrap_err();

    let diagnostic = asset_index_diagnostic(&err);
    assert_eq!(
        diagnostic.code,
        release_asset_index::AssetIndexDiagnosticCode::StaleAssetIndex
    );
    assert_eq!(diagnostic.available_m80_versions, vec!["v9.9.9"]);
    assert_eq!(
        diagnostic.available_tuples,
        vec!["linux/x86_64/minimal@v9.9.9"]
    );
    assert_eq!(
        diagnostic.repair_url.as_deref(),
        Some("https://github.com/moradology/m80/releases/download/v9.9.9/install.sh")
    );
}

#[test]
fn release_tag_source_rejects_wrong_tag_index_entry() {
    let identity = VersionIdentity::from_parts(
        "1.2.3",
        Some("v1.2.3"),
        Some("0123456789abcdef0123456789abcdef01234567"),
    );
    let temp = tempfile::tempdir().unwrap();
    let index_url = write_index_with_sidecar(
        temp.path(),
        &index_json("v1.2.3", asset_json("linux", "x86_64", "minimal", "v9.9.9")),
    );

    let err =
        source_plan_from_test_index(InstallSource::ReleaseTag("v1.2.3"), &identity, &index_url)
            .unwrap_err();

    assert!(
        err.to_string().contains("does not match index tag"),
        "{err}"
    );
    assert_eq!(
        asset_index_diagnostic(&err).code,
        release_asset_index::AssetIndexDiagnosticCode::AssetReleaseTagMismatch
    );
}

#[test]
fn asset_index_json_payload_covers_cli_path_failure_codes() {
    let identity = VersionIdentity::from_parts(
        "1.2.3",
        Some("v1.2.3"),
        Some("0123456789abcdef0123456789abcdef01234567"),
    );

    let cases = [
        (
            "missing_default_bundle",
            source_plan_error_from_index(&identity, index_json_with_assets(String::new())),
        ),
        (
            "duplicate_default_bundle",
            source_plan_error_from_index(
                &identity,
                index_json_with_assets(format!(
                    "{},{}",
                    asset_json("linux", "x86_64", "minimal", "v1.2.3"),
                    asset_json("linux", "x86_64", "minimal", "v1.2.3")
                )),
            ),
        ),
        (
            "unsupported_host_tuple",
            source_plan_error_from_index(
                &identity,
                index_json_with_assets(asset_json("linux", "aarch64", "minimal", "v1.2.3")),
            ),
        ),
        (
            "missing_image_kind",
            source_plan_error_from_index(
                &identity,
                index_json_with_assets(asset_json("linux", "x86_64", "ubuntu", "v1.2.3")),
            ),
        ),
        (
            "stale_asset_index",
            source_plan_error_from_index(
                &identity,
                index_json_with_assets(asset_json_with_m80_version(
                    "linux", "x86_64", "minimal", "v1.2.3", "v9.9.9",
                )),
            ),
        ),
        (
            "asset_release_tag_mismatch",
            source_plan_error_from_index(
                &identity,
                index_json("v1.2.3", asset_json("linux", "x86_64", "minimal", "v9.9.9")),
            ),
        ),
    ];

    for (code, err) in cases {
        let payload = asset_index_json_payload(&err);
        assert_eq!(payload["variant"], "ReleaseAssetIndex", "{payload}");
        assert_eq!(
            payload["exit_code"],
            crate::errors::EXIT_CONFIG,
            "{payload}"
        );
        assert_eq!(payload["code"], code, "{payload}");
        assert_eq!(payload["requested_os"], "linux", "{payload}");
        assert_eq!(payload["requested_arch"], "x86_64", "{payload}");
        assert_eq!(payload["requested_image_kind"], "minimal", "{payload}");
        assert_eq!(payload["requested_release_tag"], "v1.2.3", "{payload}");
        assert_eq!(payload["requested_m80_version"], "v1.2.3", "{payload}");
    }

    let dev = install_plan(
        &args_with_release_tag("v1.2.3"),
        &VersionIdentity::from_parts("1.2.3", None, None),
    )
    .unwrap_err();
    assert_eq!(asset_index_json_payload(&dev)["code"], "dev_build_refused");

    let mismatch = install_plan(&args_with_release_tag("v9.9.9"), &identity).unwrap_err();
    assert_eq!(
        asset_index_json_payload(&mismatch)["code"],
        "binary_tag_mismatch"
    );
}

#[test]
fn asset_index_fetch_failure_leaves_install_root_absent_for_dry_run_and_apply() {
    let identity = VersionIdentity::from_parts(
        "1.2.3",
        Some("v1.2.3"),
        Some("0123456789abcdef0123456789abcdef01234567"),
    );
    let temp = tempfile::tempdir().unwrap();
    let index_url = format!(
        "file://{}",
        temp.path().join("missing-release-assets.json").display()
    );

    for dry_run in [true, false] {
        let install_root = temp.path().join(format!(
            "install-root-{}",
            if dry_run { "dry" } else { "apply" }
        ));
        let mut args = args_with_release_tag("v1.2.3");
        args.install_root = install_root.clone();
        args.dry_run = dry_run;

        let err = install_plan_with_index_resolver(&args, &identity, |tag, identity| {
            release_asset_index::select_release_bundle_for_install_from_index_url(
                tag, identity, &index_url,
            )
        })
        .unwrap_err();

        assert_eq!(
            asset_index_diagnostic(&err).code,
            release_asset_index::AssetIndexDiagnosticCode::LocalReadFailed
        );
        let payload = asset_index_json_payload(&err);
        assert_eq!(payload["index_url"], index_url, "{payload}");
        assert_eq!(payload["fetch_url"], index_url, "{payload}");
        assert_eq!(payload["checksum_verification"], "before", "{payload}");
        assert!(
            !install_root.exists(),
            "asset-index failures must happen before install-root mutation for dry_run={dry_run}"
        );
    }
}

#[test]
fn asset_index_timeout_leaves_install_root_absent_for_dry_run_and_apply() {
    let _guard = super::layout::INSTALL_PREFLIGHT_ENV_LOCK
        .lock()
        .expect("process env lock poisoned");
    let identity = VersionIdentity::from_parts(
        "1.2.3",
        Some("v1.2.3"),
        Some("0123456789abcdef0123456789abcdef01234567"),
    );
    let temp = tempfile::tempdir().unwrap();

    for dry_run in [true, false] {
        let server = SlowIndexServer::new();
        let index_url = server.url("/m80-release-assets.json");
        let install_root = temp.path().join(format!(
            "timeout-install-root-{}",
            if dry_run { "dry" } else { "apply" }
        ));
        let mut args = args_with_release_tag("v1.2.3");
        args.install_root = install_root.clone();
        args.dry_run = dry_run;

        let err = install_plan_with_index_resolver(&args, &identity, |tag, identity| {
            release_asset_index::select_release_bundle_for_install_from_index_url_with_download_bounds(
                tag,
                identity,
                &index_url,
                1,
                1,
            )
        })
        .unwrap_err();

        let payload = asset_index_json_payload(&err);
        assert_eq!(payload["code"], "download_failed", "{payload}");
        assert_eq!(payload["index_url"], index_url, "{payload}");
        assert_eq!(payload["fetch_url"], index_url, "{payload}");
        assert_eq!(payload["checksum_verification"], "before", "{payload}");
        assert!(err.to_string().contains("failure=timeout"), "{err}");
        assert!(
            !install_root.exists(),
            "asset-index timeout must happen before install-root mutation for dry_run={dry_run}"
        );
    }
}

#[test]
fn release_tag_source_refuses_downgrade_before_index_fetch() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let install_root = temp.path().join("install-root");
    seed_active_release(&install_root, "v1.2.3");
    let identity = VersionIdentity::from_parts(
        "1.2.2",
        Some("v1.2.2"),
        Some("0123456789abcdef0123456789abcdef01234567"),
    );
    let mut args = args_with_release_tag("v1.2.2");
    args.install_root = install_root.clone();

    let err = install_plan_with_index_resolver(&args, &identity, |_, _| {
        panic!("downgrade refusal must happen before asset-index fetch")
    })
    .expect_err("older release should be refused before asset-index fetch");

    let report = release_transition_report(&err);
    assert_eq!(report.state, ReleaseTransitionState::DowngradeRefused);
    assert_eq!(report.active_tag.as_deref(), Some("v1.2.3"));
    assert_eq!(report.target_tag.as_deref(), Some("v1.2.2"));
    assert_eq!(report.observed_ordering, "target_older");
    assert!(
        !install_root.join(".staging").exists(),
        "downgrade refusal must not create installer staging"
    );
}

#[test]
fn official_bundle_url_refuses_downgrade_before_attestation_preflight() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let install_root = temp.path().join("install-root");
    seed_active_release(&install_root, "v1.2.3");
    let identity = VersionIdentity::from_parts(
        "1.2.2",
        Some("v1.2.2"),
        Some("0123456789abcdef0123456789abcdef01234567"),
    );
    let mut args = args_with_bundle_url(
        "https://github.com/moradology/m80/releases/download/v1.2.2/m80-linux-x86_64.tar.gz",
    );
    args.install_root = install_root.clone();

    let err = install_plan(&args, &identity)
        .expect_err("older official URL should be refused before attestation preflight");

    let report = release_transition_report(&err);
    assert_eq!(report.state, ReleaseTransitionState::DowngradeRefused);
    assert_eq!(report.active_tag.as_deref(), Some("v1.2.3"));
    assert_eq!(report.target_tag.as_deref(), Some("v1.2.2"));
    assert!(
        !install_root.join(".staging").exists(),
        "official URL downgrade refusal must not create installer staging"
    );
}

#[test]
fn release_transition_json_payload_carries_downgrade_tags() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let install_root = temp.path().join("install-root");
    seed_active_release(&install_root, "v1.2.3");

    let err = enforce_release_transition_for_target(Some(&install_root), "v1.2.2")
        .expect_err("older target should be refused");
    let payload = release_transition_json_payload(&err);

    assert_eq!(payload["variant"], "ReleaseTransition", "{payload}");
    assert_eq!(
        payload["exit_code"],
        crate::errors::EXIT_CONFIG,
        "{payload}"
    );
    assert_eq!(payload["code"], "downgrade_refused", "{payload}");
    assert_eq!(payload["active_tag"], "v1.2.3", "{payload}");
    assert_eq!(payload["requested_tag"], "v1.2.2", "{payload}");
    assert_eq!(payload["observed_ordering"], "target_older", "{payload}");
    assert!(
        payload["expected_ordering"]
            .as_str()
            .is_some_and(|value| value.contains("newer than or equal")),
        "{payload}"
    );
    assert_eq!(
        payload["reinstall_active_command"],
        "curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.3/install.sh | sudo sh",
        "{payload}"
    );
    assert!(payload["rollback_command"].is_null(), "{payload}");
}

#[test]
fn same_version_reinstall_is_not_downgrade_refused() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let install_root = temp.path().join("install-root");
    seed_active_release(&install_root, "v1.2.3");

    enforce_release_transition_for_target(Some(&install_root), "v1.2.3")
        .expect("same-version target should not be downgrade refused");
}

#[test]
fn human_layout_summary_includes_reinstall_diagnostics() {
    let lines = layout_summary_lines(&layout::reinstall_summary_for_render_test());

    assert!(
        lines.contains(&"release already installed".to_owned()),
        "{lines:?}"
    );
    assert!(
        lines.contains(&"install_state=already_installed".to_owned()),
        "{lines:?}"
    );
    assert!(
        lines.contains(&"next_command=m80 run -- echo hello".to_owned()),
        "{lines:?}"
    );
    assert!(
        lines.contains(&"reinstall_status=idempotent_same_material".to_owned()),
        "{lines:?}"
    );
    assert!(
        lines.contains(&"existing_proof_cache_manifest_digest=existing-digest".to_owned()),
        "{lines:?}"
    );
    assert!(
        lines.contains(&"verified_proof_cache_manifest_digest=verified-digest".to_owned()),
        "{lines:?}"
    );
}

#[test]
fn missing_active_metadata_still_refuses_older_target_by_pointer_tag() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let install_root = temp.path().join("install-root");
    let active_version = install_root.join("versions/v1.2.3");
    fs::create_dir_all(&active_version).expect("create active version dir");
    symlink(&active_version, install_root.join("active")).expect("point active at version");

    let err = enforce_release_transition_for_target(Some(&install_root), "v1.2.2")
        .expect_err("missing metadata must not bypass downgrade refusal");

    let report = release_transition_report(&err);
    assert_eq!(report.state, ReleaseTransitionState::DowngradeRefused);
    assert_eq!(report.active_tag.as_deref(), Some("v1.2.3"));
    assert_eq!(report.target_tag.as_deref(), Some("v1.2.2"));
}

#[test]
fn missing_active_pointer_does_not_block_first_install() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let install_root = temp.path().join("install-root");

    enforce_release_transition_for_target(Some(&install_root), "v1.2.2")
        .expect("missing active pointer should be treated as first install");
}

#[test]
fn release_tag_source_rejects_dev_binary() {
    let identity = VersionIdentity::from_parts("1.2.3", None, None);
    let err = install_plan(&args_with_release_tag("v1.2.3"), &identity).unwrap_err();

    assert!(
        err.to_string().contains("requires a tagged m80 binary"),
        "{err}"
    );
    let diagnostic = asset_index_diagnostic(&err);
    assert_eq!(
        diagnostic.code,
        release_asset_index::AssetIndexDiagnosticCode::DevBuildRefused
    );
    assert_eq!(diagnostic.requested_release_tag, "v1.2.3");
}

#[test]
fn release_tag_source_rejects_binary_tag_mismatch() {
    let identity = VersionIdentity::from_parts(
        "1.2.3",
        Some("v1.2.3"),
        Some("0123456789abcdef0123456789abcdef01234567"),
    );
    let err = install_plan(&args_with_release_tag("v9.9.9"), &identity).unwrap_err();

    assert!(
        err.to_string().contains("bundle/binary tag mismatch"),
        "{err}"
    );
    let diagnostic = asset_index_diagnostic(&err);
    assert_eq!(
        diagnostic.code,
        release_asset_index::AssetIndexDiagnosticCode::BinaryTagMismatch
    );
    assert_eq!(diagnostic.requested_release_tag, "v9.9.9");
    assert_eq!(
        diagnostic.repair_url.as_deref(),
        Some("https://github.com/moradology/m80/releases/download/v1.2.3/install.sh")
    );
}

#[test]
fn release_tag_source_rejects_prerelease_tag_before_index_fetch() {
    let identity = VersionIdentity::from_parts(
        "1.2.3-rc.1",
        Some("v1.2.3-rc.1"),
        Some("0123456789abcdef0123456789abcdef01234567"),
    );
    let err = source_plan_with_index_resolver(
        InstallSource::ReleaseTag("v1.2.3-rc.1"),
        &identity,
        |_, _| panic!("prerelease-shaped tag must fail before index fetch"),
    )
    .unwrap_err();

    assert!(err.to_string().contains("no prerelease suffix"), "{err}");
}

#[test]
fn bootstrap_tag_source_rejects_prerelease_tag_before_index_fetch() {
    let identity = VersionIdentity::from_parts(
        "1.2.3-rc.1",
        Some("v1.2.3-rc.1"),
        Some("0123456789abcdef0123456789abcdef01234567"),
    );
    let err = source_plan_with_index_resolver(
        InstallSource::BootstrapTag("v1.2.3-rc.1"),
        &identity,
        |_, _| panic!("prerelease-shaped bootstrap tag must fail before index fetch"),
    )
    .unwrap_err();

    assert!(err.to_string().contains("no prerelease suffix"), "{err}");
}

#[test]
fn bundle_url_rejects_release_binary_tag_mismatch() {
    let identity = VersionIdentity::from_parts(
        "1.2.3",
        Some("v1.2.3"),
        Some("0123456789abcdef0123456789abcdef01234567"),
    );
    let bundle_tag = release_tag_from_bundle_url(
        "https://github.com/moradology/m80/releases/download/v9.9.9/m80-linux-x86_64.tar.gz",
    );
    let err = validate_bundle_url_matches_binary(bundle_tag.as_deref(), &identity).unwrap_err();

    assert!(
        err.to_string().contains("bundle/binary tag mismatch"),
        "{err}"
    );
}

#[test]
fn tagged_release_bundle_url_rejects_dev_binary() {
    let identity = VersionIdentity::from_parts("1.2.3", None, None);
    let bundle_tag = release_tag_from_bundle_url(
        "https://github.com/moradology/m80/releases/download/v1.2.3/m80-linux-x86_64.tar.gz",
    );
    let err = validate_bundle_url_matches_binary(bundle_tag.as_deref(), &identity).unwrap_err();

    assert!(
        err.to_string().contains("GitHub release bundle URL"),
        "{err}"
    );
}

#[test]
fn explicit_bundle_url_allows_dev_binary_for_local_bundle_testing() {
    let identity = VersionIdentity::from_parts("1.2.3", None, None);
    let plan = install_plan(
        &args_with_bundle_url("file:///tmp/m80-linux-x86_64.tar.gz"),
        &identity,
    )
    .unwrap();

    assert_eq!(plan.source.kind, SourceKind::BundleUrl);
    assert_eq!(
        plan.source.bundle_url.as_deref(),
        Some("file:///tmp/m80-linux-x86_64.tar.gz")
    );
    assert_eq!(plan.version_status, "dev");
}

#[test]
fn explicit_bundle_url_does_not_require_index_selection() {
    let identity = VersionIdentity::from_parts("1.2.3", None, None);
    let plan = source_plan_with_index_resolver(
        InstallSource::BundleUrl("file:///tmp/m80-linux-x86_64.tar.gz"),
        &identity,
        |_, _| panic!("explicit bundle URL must bypass index selection"),
    )
    .unwrap();

    assert_eq!(plan.kind, SourceKind::BundleUrl);
    assert_eq!(
        plan.bundle_url.as_deref(),
        Some("file:///tmp/m80-linux-x86_64.tar.gz")
    );
}

#[test]
fn release_bundle_tag_is_extracted_from_github_url() {
    let tag = release_tag_from_bundle_url(
        "https://github.com/moradology/m80/releases/download/v1.2.3/m80-linux-x86_64.tar.gz",
    );

    assert_eq!(tag.as_deref(), Some("v1.2.3"));
}

#[test]
fn local_fixture_release_shaped_url_does_not_select_release_tag() {
    let tag = release_tag_from_bundle_url(
        "http://127.0.0.1/releases/download/v1.2.3/m80-linux-x86_64.tar.gz",
    );

    assert_eq!(tag, None);
}

fn asset_index_diagnostic(err: &InstallError) -> &release_asset_index::AssetIndexDiagnostic {
    match err {
        InstallError::AssetIndex(err) => err.diagnostic(),
        InstallError::Fc(err) => panic!("expected asset-index error, got {err}"),
        InstallError::ReleaseTransition(err) => {
            panic!("expected asset-index error, got release transition {err:?}")
        }
    }
}

fn asset_index_json_payload(err: &InstallError) -> serde_json::Value {
    match err {
        InstallError::AssetIndex(err) => {
            serde_json::to_value(asset_index_error_payload(err)).unwrap()
        }
        InstallError::Fc(err) => panic!("expected asset-index error, got {err}"),
        InstallError::ReleaseTransition(err) => {
            panic!("expected asset-index error, got release transition {err:?}")
        }
    }
}

fn release_transition_report(err: &InstallError) -> &ReleaseTransitionReport {
    match err {
        InstallError::ReleaseTransition(report) => report,
        InstallError::Fc(err) => panic!("expected release-transition error, got {err}"),
        InstallError::AssetIndex(err) => {
            panic!("expected release-transition error, got asset index {err}")
        }
    }
}

fn release_transition_json_payload(err: &InstallError) -> serde_json::Value {
    match err {
        InstallError::ReleaseTransition(report) => {
            serde_json::to_value(release_transition_error_payload(report))
                .expect("release transition payload should serialize")
        }
        InstallError::Fc(err) => panic!("expected release-transition error, got {err}"),
        InstallError::AssetIndex(err) => {
            panic!("expected release-transition error, got asset index {err}")
        }
    }
}

fn seed_active_release(install_root: &Path, tag: &str) {
    let version_dir = install_root.join("versions").join(tag);
    fs::create_dir_all(&version_dir).expect("create active version dir");
    symlink(&version_dir, install_root.join("active")).expect("point active at version");
}

fn source_plan_error_from_index(identity: &VersionIdentity, json: String) -> InstallError {
    let temp = tempfile::tempdir().unwrap();
    let index_url = write_index_with_sidecar(temp.path(), &json);
    source_plan_from_test_index(InstallSource::ReleaseTag("v1.2.3"), identity, &index_url)
        .unwrap_err()
}

fn source_plan_from_test_index(
    source: InstallSource<'_>,
    identity: &VersionIdentity,
    index_url: &str,
) -> Result<SourcePlan, InstallError> {
    source_plan_with_index_resolver(source, identity, |tag, identity| {
        release_asset_index::select_release_bundle_for_install_from_index_url(
            tag, identity, index_url,
        )
    })
}

fn write_index_with_sidecar(root: &Path, json: &str) -> String {
    let index = root.join("m80-release-assets.json");
    fs::write(&index, json).unwrap();
    fs::write(
        root.join("m80-release-assets.json.sha256"),
        format!("{}  m80-release-assets.json\n", sha256(json.as_bytes())),
    )
    .unwrap();
    format!("file://{}", index.display())
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn index_json_with_assets(assets: String) -> String {
    index_json("v1.2.3", assets)
}

fn index_json(release_tag: &str, assets: String) -> String {
    format!(
        r#"{{
  "schema_version": 1,
  "release_tag": "{release_tag}",
  "assets": [{assets}]
}}"#
    )
}

fn asset_json(os: &str, arch: &str, image_kind: &str, release_tag: &str) -> String {
    asset_json_with_m80_version(os, arch, image_kind, release_tag, release_tag)
}

fn asset_json_with_m80_version(
    os: &str,
    arch: &str,
    image_kind: &str,
    release_tag: &str,
    m80_version: &str,
) -> String {
    let target = format!("{os}-{arch}");
    format!(
        r#"{{
  "name": "m80-{target}.tar.gz",
  "url": "https://github.com/moradology/m80/releases/download/{release_tag}/m80-{target}.tar.gz",
  "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "size_bytes": 42,
  "metadata_name": "m80-{target}.bundle.json",
  "metadata_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
  "checksum_name": "m80-{target}.tar.gz.sha256",
  "signature_name": "m80.sig",
  "attestation_name": "m80.intoto.jsonl",
  "target": "{target}",
  "os": "{os}",
  "arch": "{arch}",
  "image_kind": "{image_kind}",
  "release_tag": "{release_tag}",
  "m80_version": "{m80_version}",
  "guest_protocol_version": 1,
  "manifest_schema_version": 1,
  "expected_firecracker_version": "v1.15.1"
}}"#
    )
}

struct SlowIndexServer {
    addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl SlowIndexServer {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_shutdown = Arc::clone(&shutdown);
        let handle = thread::spawn(move || {
            while !thread_shutdown.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => handle_slow_index_request(&mut stream),
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            addr,
            shutdown,
            handle: Some(handle),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }
}

impl Drop for SlowIndexServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = std::net::TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            handle.join().unwrap();
        }
    }
}

fn handle_slow_index_request(stream: &mut std::net::TcpStream) {
    let mut request = [0_u8; 256];
    let _ = stream.read(&mut request);
    thread::sleep(Duration::from_secs(2));
    let body = b"{}";
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
}
