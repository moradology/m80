use std::fs;
use std::path::Path;

use m80_firecracker::FcError;
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
    let identity = VersionIdentity::from_parts("1.2.3", Some("v1.2.3"));
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
    let identity = VersionIdentity::from_parts("1.2.3", Some("v1.2.3"));
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
    let identity = VersionIdentity::from_parts("1.2.3", Some("v1.2.3"));
    let temp = tempfile::tempdir().unwrap();
    let index_url = write_index_with_sidecar(temp.path(), &index_json_with_assets(String::new()));

    let err =
        source_plan_from_test_index(InstallSource::ReleaseTag("v1.2.3"), &identity, &index_url)
            .unwrap_err();

    assert!(err.to_string().contains("no default bundle"), "{err}");
}

#[test]
fn release_tag_source_rejects_duplicate_default_index_entry() {
    let identity = VersionIdentity::from_parts("1.2.3", Some("v1.2.3"));
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
}

#[test]
fn release_tag_source_rejects_wrong_arch_index_entry() {
    let identity = VersionIdentity::from_parts("1.2.3", Some("v1.2.3"));
    let temp = tempfile::tempdir().unwrap();
    let index_url = write_index_with_sidecar(
        temp.path(),
        &index_json_with_assets(asset_json("linux", "aarch64", "minimal", "v1.2.3")),
    );

    let err =
        source_plan_from_test_index(InstallSource::ReleaseTag("v1.2.3"), &identity, &index_url)
            .unwrap_err();

    assert!(err.to_string().contains("os=linux arch=x86_64"), "{err}");
}

#[test]
fn release_tag_source_rejects_wrong_image_kind_index_entry() {
    let identity = VersionIdentity::from_parts("1.2.3", Some("v1.2.3"));
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
}

#[test]
fn release_tag_source_rejects_wrong_tag_index_entry() {
    let identity = VersionIdentity::from_parts("1.2.3", Some("v1.2.3"));
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
}

#[test]
fn release_tag_source_rejects_dev_binary() {
    let identity = VersionIdentity::from_parts("1.2.3", None);
    let err = install_plan(&args_with_release_tag("v1.2.3"), &identity).unwrap_err();

    assert!(
        err.to_string().contains("requires a tagged m80 binary"),
        "{err}"
    );
}

#[test]
fn release_tag_source_rejects_binary_tag_mismatch() {
    let identity = VersionIdentity::from_parts("1.2.3", Some("v1.2.3"));
    let err = install_plan(&args_with_release_tag("v9.9.9"), &identity).unwrap_err();

    assert!(
        err.to_string().contains("bundle/binary tag mismatch"),
        "{err}"
    );
}

#[test]
fn bundle_url_rejects_release_binary_tag_mismatch() {
    let identity = VersionIdentity::from_parts("1.2.3", Some("v1.2.3"));
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
    let identity = VersionIdentity::from_parts("1.2.3", None);
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
    let identity = VersionIdentity::from_parts("1.2.3", None);
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
    let identity = VersionIdentity::from_parts("1.2.3", None);
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

fn source_plan_from_test_index(
    source: InstallSource<'_>,
    identity: &VersionIdentity,
    index_url: &str,
) -> Result<SourcePlan, FcError> {
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
  "m80_version": "v1.2.3",
  "guest_protocol_version": 1,
  "manifest_schema_version": 1,
  "expected_firecracker_version": "v1.15.1"
}}"#
    )
}
