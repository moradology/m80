use std::fs;
use std::path::Path;

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
    let err = install_plan(
        &args_with_bundle_url("http://127.0.0.1/releases/download/v9.9.9/m80-linux-x86_64.tar.gz"),
        &identity,
    )
    .unwrap_err();

    assert!(
        err.to_string().contains("bundle/binary tag mismatch"),
        "{err}"
    );
}

#[test]
fn tagged_release_bundle_url_rejects_dev_binary() {
    let identity = VersionIdentity::from_parts("1.2.3", None, None);
    let err = install_plan(
        &args_with_bundle_url("http://127.0.0.1/releases/download/v1.2.3/m80-linux-x86_64.tar.gz"),
        &identity,
    )
    .unwrap_err();

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

fn asset_index_diagnostic(err: &InstallError) -> &release_asset_index::AssetIndexDiagnostic {
    match err {
        InstallError::AssetIndex(err) => err.diagnostic(),
        InstallError::Fc(err) => panic!("expected asset-index error, got {err}"),
    }
}

fn asset_index_json_payload(err: &InstallError) -> serde_json::Value {
    match err {
        InstallError::AssetIndex(err) => {
            serde_json::to_value(asset_index_error_payload(err)).unwrap()
        }
        InstallError::Fc(err) => panic!("expected asset-index error, got {err}"),
    }
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
