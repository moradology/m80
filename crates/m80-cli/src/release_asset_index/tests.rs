use super::fetch::{
    fetch_verified_asset_index, github_release_asset_index_url, sha256_bytes, AssetIndexFetchError,
    AssetIndexFetchRequest,
};
use super::*;
use std::fs;
use std::path::Path;

#[test]
fn parser_round_trip_accepts_default_linux_bundle() {
    let index = valid_index();

    assert_eq!(index.schema_version, 1);
    assert_eq!(index.release_tag, "v0.0.0");
    assert_eq!(index.assets.len(), 1);
    assert_eq!(index.assets[0].os, "linux");
    assert_eq!(index.assets[0].arch, "x86_64");
    assert_eq!(index.assets[0].image_kind, "minimal");
    assert_eq!(index.assets[0].signature_name.as_deref(), Some("m80.sig"));
    assert_eq!(
        index.assets[0].attestation_name.as_deref(),
        Some("m80.intoto.jsonl")
    );
}

#[test]
fn verified_file_index_fetch_accepts_valid_checksum_before_parse() {
    let temp = tempfile::tempdir().unwrap();
    let index_url = write_index_with_sidecar(temp.path(), &index_json_with_schema(1));

    let fetched = fetch_verified_asset_index(fetch_request(&index_url)).unwrap();

    assert_eq!(fetched.index.release_tag, "v0.0.0");
    assert_eq!(fetched.index_url, index_url);
    assert_eq!(fetched.checksum_url, format!("{index_url}.sha256"));
    assert_eq!(fetched.expected_sha256, fetched.observed_sha256);
    assert_eq!(fetched.index.assets[0].name, "m80-linux-x86_64.tar.gz");
}

#[test]
fn verified_file_index_fetch_names_missing_index_with_context() {
    let temp = tempfile::tempdir().unwrap();
    let index = temp.path().join(ASSET_INDEX_NAME);
    let index_url = file_url(&index);

    let err = fetch_verified_asset_index(fetch_request(&index_url)).unwrap_err();

    let message = err.to_string();
    assert!(message.contains(index.to_str().unwrap()), "{message}");
    assert!(message.contains("release_tag=v0.0.0"), "{message}");
    assert!(message.contains("host=linux/x86_64"), "{message}");
    assert!(message.contains("image_kind=minimal"), "{message}");
}

#[test]
fn verified_file_index_fetch_names_missing_checksum_sidecar() {
    let temp = tempfile::tempdir().unwrap();
    let index = temp.path().join(ASSET_INDEX_NAME);
    fs::write(&index, index_json_with_schema(1)).unwrap();
    let index_url = file_url(&index);

    let err = fetch_verified_asset_index(fetch_request(&index_url)).unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("m80-release-assets.json.sha256"),
        "{message}"
    );
    assert!(message.contains("release_tag=v0.0.0"), "{message}");
}

#[test]
fn verified_file_index_fetch_rejects_bad_checksum_before_json_parse() {
    let temp = tempfile::tempdir().unwrap();
    let index = temp.path().join(ASSET_INDEX_NAME);
    fs::write(&index, "{not json").unwrap();
    fs::write(
        temp.path().join(format!("{ASSET_INDEX_NAME}.sha256")),
        format!("{}  {ASSET_INDEX_NAME}\n", "0".repeat(64)),
    )
    .unwrap();
    let index_url = file_url(&index);

    let err = fetch_verified_asset_index(fetch_request(&index_url)).unwrap_err();

    assert!(matches!(err, AssetIndexFetchError::ChecksumMismatch { .. }));
    let message = err.to_string();
    assert!(message.contains("expected"), "{message}");
    assert!(message.contains("observed"), "{message}");
    assert!(!message.contains("JSON is invalid"), "{message}");
}

#[test]
fn verified_file_index_fetch_rejects_invalid_json_after_valid_checksum() {
    let temp = tempfile::tempdir().unwrap();
    let index_url = write_index_with_sidecar(temp.path(), "{not json");

    let err = fetch_verified_asset_index(fetch_request(&index_url)).unwrap_err();

    assert!(matches!(
        err,
        AssetIndexFetchError::VerifiedIndexInvalid { .. }
    ));
    let message = err.to_string();
    assert!(
        message.contains("verified release asset index invalid"),
        "{message}"
    );
    assert!(message.contains("expected_sha256="), "{message}");
    assert!(message.contains("observed_sha256="), "{message}");
    assert!(message.contains("release_tag=v0.0.0"), "{message}");
}

#[test]
fn verified_file_index_fetch_rejects_stale_schema_after_valid_checksum() {
    let temp = tempfile::tempdir().unwrap();
    let index_url = write_index_with_sidecar(temp.path(), &index_json_with_schema(999));

    let err = fetch_verified_asset_index(fetch_request(&index_url)).unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("schema mismatch: expected 1, got 999"),
        "{message}"
    );
    assert!(message.contains("expected_sha256="), "{message}");
    assert!(message.contains("host=linux/x86_64"), "{message}");
}

#[test]
fn verified_file_index_fetch_rejects_index_release_tag_mismatch() {
    let temp = tempfile::tempdir().unwrap();
    let index_url = write_index_with_sidecar(
        temp.path(),
        &index_json(
            "v9.9.9",
            asset_json("linux", "x86_64", "minimal", "v9.9.9", "v9.9.9"),
        ),
    );

    let err = fetch_verified_asset_index(fetch_request(&index_url)).unwrap_err();

    assert!(matches!(
        err,
        AssetIndexFetchError::ReleaseTagMismatch { .. }
    ));
    let message = err.to_string();
    assert!(message.contains("expected release_tag v0.0.0"), "{message}");
    assert!(message.contains("got v9.9.9"), "{message}");
    assert!(message.contains("observed_sha256="), "{message}");
}

#[test]
fn pinned_github_asset_index_url_uses_moradology_m80_release() {
    assert_eq!(
        github_release_asset_index_url("v1.2.3"),
        "https://github.com/moradology/m80/releases/download/v1.2.3/m80-release-assets.json"
    );
}

#[test]
fn host_tuple_selection_picks_linux_x86_64_minimal() {
    let index = valid_index();
    let selection = index
        .select_default_bundle(release_binary(), linux_x86_64(), "minimal")
        .unwrap();

    assert_eq!(selection.name, "m80-linux-x86_64.tar.gz");
    assert_eq!(
        selection.url,
        "https://github.com/moradology/m80/releases/download/v0.0.0/m80-linux-x86_64.tar.gz"
    );
}

#[test]
fn explicit_bundle_url_bypasses_index_selection() {
    let index = valid_index();
    let selection = index
        .resolve_bundle(BundleSelectionRequest {
            binary: dev_binary(),
            host: HostTuple {
                os: "plan9",
                arch: "mips",
            },
            image_kind: Some("ubuntu"),
            explicit_bundle_url: Some("file:///tmp/local-bundle.tar.gz"),
        })
        .unwrap();

    assert_eq!(
        selection,
        BundleSelection::ExplicitUrl("file:///tmp/local-bundle.tar.gz")
    );
}

#[test]
fn duplicate_default_fails_closed() {
    let json = index_json_with_assets(format!(
        "{},{}",
        asset_json("linux", "x86_64", "minimal", "v0.0.0", "v0.0.0"),
        asset_json("linux", "x86_64", "minimal", "v0.0.0", "v0.0.0")
    ));
    let index = ReleaseAssetIndex::parse_json(&json).unwrap();

    let err = index
        .select_default_bundle(release_binary(), linux_x86_64(), "minimal")
        .unwrap_err();

    assert!(matches!(err, AssetIndexError::DuplicateDefault { .. }));
}

#[test]
fn missing_default_fails_closed() {
    let err = ReleaseAssetIndex::parse_json(&index_json_with_assets(String::new())).unwrap_err();

    assert!(matches!(err, AssetIndexError::MissingDefault { .. }));
}

#[test]
fn wrong_architecture_fails_closed() {
    let json = index_json_with_assets(asset_json(
        "linux", "aarch64", "minimal", "v0.0.0", "v0.0.0",
    ));
    let index = ReleaseAssetIndex::parse_json(&json).unwrap();

    let err = index
        .select_default_bundle(release_binary(), linux_x86_64(), "minimal")
        .unwrap_err();

    assert!(matches!(err, AssetIndexError::WrongArchitecture { .. }));
}

#[test]
fn wrong_image_kind_fails_closed() {
    let json = index_json_with_assets(asset_json("linux", "x86_64", "ubuntu", "v0.0.0", "v0.0.0"));
    let index = ReleaseAssetIndex::parse_json(&json).unwrap();

    let err = index
        .select_default_bundle(release_binary(), linux_x86_64(), "minimal")
        .unwrap_err();

    assert!(matches!(err, AssetIndexError::WrongImageKind { .. }));
}

#[test]
fn wrong_tag_fails_closed() {
    let json = index_json(
        "v9.9.9",
        asset_json("linux", "x86_64", "minimal", "v9.9.9", "v9.9.9"),
    );
    let index = ReleaseAssetIndex::parse_json(&json).unwrap();

    let err = index
        .select_default_bundle(release_binary(), linux_x86_64(), "minimal")
        .unwrap_err();

    assert!(matches!(err, AssetIndexError::WrongTag { .. }));
}

#[test]
fn wrong_m80_version_fails_closed() {
    let json = index_json_with_assets(asset_json("linux", "x86_64", "minimal", "v0.0.0", "v9.9.9"));
    let index = ReleaseAssetIndex::parse_json(&json).unwrap();

    let err = index
        .select_default_bundle(release_binary(), linux_x86_64(), "minimal")
        .unwrap_err();

    assert!(matches!(err, AssetIndexError::WrongM80Version { .. }));
}

#[test]
fn dev_build_selection_fails_closed() {
    let index = valid_index();

    let err = index
        .select_default_bundle(dev_binary(), linux_x86_64(), "minimal")
        .unwrap_err();

    assert_eq!(
        err,
        AssetIndexError::DevBuildSelection {
            m80_version: "0.0.0-dev".to_owned()
        }
    );
}

#[test]
fn mismatched_build_selection_fails_closed() {
    let index = valid_index();

    let err = index
        .select_default_bundle(mismatch_binary(), linux_x86_64(), "minimal")
        .unwrap_err();

    assert!(matches!(
        err,
        AssetIndexError::MismatchedBuildSelection { .. }
    ));
}

#[test]
fn wrong_architecture_diagnostic_names_requested_and_available_tuple() {
    let json = index_json_with_assets(asset_json(
        "linux", "aarch64", "minimal", "v0.0.0", "v0.0.0",
    ));
    let index = ReleaseAssetIndex::parse_json(&json).unwrap();

    let err = index
        .select_default_bundle(release_binary(), linux_x86_64(), "minimal")
        .unwrap_err()
        .to_string();

    assert!(err.contains("os=linux arch=x86_64"), "{err}");
    assert!(err.contains("linux/aarch64/minimal@v0.0.0"), "{err}");
    assert!(err.contains("--bundle-url"), "{err}");
}

#[test]
fn wrong_image_kind_diagnostic_names_requested_and_available_kind() {
    let json = index_json_with_assets(asset_json("linux", "x86_64", "ubuntu", "v0.0.0", "v0.0.0"));
    let index = ReleaseAssetIndex::parse_json(&json).unwrap();

    let err = index
        .select_default_bundle(release_binary(), linux_x86_64(), "minimal")
        .unwrap_err()
        .to_string();

    assert!(err.contains("requested image kind minimal"), "{err}");
    assert!(err.contains("available image kinds: ubuntu"), "{err}");
    assert!(err.contains("--bundle-url"), "{err}");
}

#[test]
fn stale_version_diagnostic_names_requested_available_and_pinned_url() {
    let json = index_json_with_assets(asset_json("linux", "x86_64", "minimal", "v0.0.0", "v9.9.9"));
    let index = ReleaseAssetIndex::parse_json(&json).unwrap();

    let err = index
        .select_default_bundle(release_binary(), linux_x86_64(), "minimal")
        .unwrap_err()
        .to_string();

    assert!(err.contains("requested m80 version v0.0.0"), "{err}");
    assert!(err.contains("available versions: v9.9.9"), "{err}");
    assert!(
        err.contains("https://github.com/moradology/m80/releases/download/v9.9.9/install.sh"),
        "{err}"
    );
}

#[test]
fn wrong_tag_diagnostic_names_binary_tag_and_pinned_url() {
    let json = index_json(
        "v9.9.9",
        asset_json("linux", "x86_64", "minimal", "v9.9.9", "v9.9.9"),
    );
    let index = ReleaseAssetIndex::parse_json(&json).unwrap();

    let err = index
        .select_default_bundle(release_binary(), linux_x86_64(), "minimal")
        .unwrap_err()
        .to_string();

    assert!(err.contains("index tag v9.9.9"), "{err}");
    assert!(err.contains("binary tag v0.0.0"), "{err}");
    assert!(
        err.contains("https://github.com/moradology/m80/releases/download/v0.0.0/install.sh"),
        "{err}"
    );
}

#[test]
fn dev_build_diagnostic_names_local_bundle_repair() {
    let index = valid_index();

    let err = index
        .select_default_bundle(dev_binary(), linux_x86_64(), "minimal")
        .unwrap_err()
        .to_string();

    assert!(err.contains("dev build 0.0.0-dev"), "{err}");
    assert!(err.contains("--bundle-url"), "{err}");
    assert!(err.contains("tagged release binary"), "{err}");
}

#[test]
fn schema_mismatch_diagnostic_names_expected_and_actual() {
    let err = ReleaseAssetIndex::parse_json(&index_json_with_schema(999)).unwrap_err();

    assert_eq!(
        err.to_string(),
        "release asset index schema mismatch: expected 1, got 999"
    );
}

#[test]
fn unknown_index_fields_fail_closed() {
    let mut value: serde_json::Value = serde_json::from_str(&index_json_with_schema(1)).unwrap();
    value["unexpected"] = serde_json::json!(true);

    let err = ReleaseAssetIndex::parse_json(&value.to_string()).unwrap_err();

    assert!(matches!(err, AssetIndexError::Json { .. }));
}

#[test]
fn asset_target_must_match_os_arch() {
    let mut value: serde_json::Value = serde_json::from_str(&index_json_with_assets(asset_json(
        "linux", "x86_64", "minimal", "v0.0.0", "v0.0.0",
    )))
    .unwrap();
    value["assets"][0]["target"] = serde_json::json!("linux-aarch64");

    let err = ReleaseAssetIndex::parse_json(&value.to_string()).unwrap_err();

    assert!(matches!(err, AssetIndexError::TargetTupleMismatch { .. }));
}

#[test]
fn bundle_digest_must_be_sha256() {
    let err =
        parse_index_with_asset_field("sha256", serde_json::json!("not-a-digest")).unwrap_err();

    assert!(matches!(
        err,
        AssetIndexError::InvalidField {
            field: "sha256",
            ..
        }
    ));
}

#[test]
fn metadata_digest_must_be_sha256() {
    let err = parse_index_with_asset_field("metadata_sha256", serde_json::json!("not-a-digest"))
        .unwrap_err();

    assert!(matches!(
        err,
        AssetIndexError::InvalidField {
            field: "metadata_sha256",
            ..
        }
    ));
}

#[test]
fn signature_name_must_not_be_empty_when_present() {
    let err = parse_index_with_asset_field("signature_name", serde_json::json!("")).unwrap_err();

    assert_eq!(
        err,
        AssetIndexError::MissingField {
            field: "signature_name"
        }
    );
}

#[test]
fn size_bytes_must_be_nonzero() {
    let err = parse_index_with_asset_field("size_bytes", serde_json::json!(0)).unwrap_err();

    assert!(matches!(
        err,
        AssetIndexError::InvalidField {
            field: "size_bytes",
            ..
        }
    ));
}

#[test]
fn guest_protocol_version_must_be_nonzero() {
    let err =
        parse_index_with_asset_field("guest_protocol_version", serde_json::json!(0)).unwrap_err();

    assert!(matches!(
        err,
        AssetIndexError::InvalidField {
            field: "guest_protocol_version",
            ..
        }
    ));
}

#[test]
fn manifest_schema_version_must_be_nonzero() {
    let err =
        parse_index_with_asset_field("manifest_schema_version", serde_json::json!(0)).unwrap_err();

    assert!(matches!(
        err,
        AssetIndexError::InvalidField {
            field: "manifest_schema_version",
            ..
        }
    ));
}

fn valid_index() -> ReleaseAssetIndex {
    ReleaseAssetIndex::parse_json(&index_json_with_schema(1)).unwrap()
}

fn release_binary() -> BinaryRelease<'static> {
    BinaryRelease {
        status: VersionStatus::Release,
        release_tag: Some("v0.0.0"),
        m80_version: "v0.0.0",
    }
}

fn dev_binary() -> BinaryRelease<'static> {
    BinaryRelease {
        status: VersionStatus::Dev,
        release_tag: None,
        m80_version: "0.0.0-dev",
    }
}

fn mismatch_binary() -> BinaryRelease<'static> {
    BinaryRelease {
        status: VersionStatus::Mismatch,
        release_tag: Some("v0.0.0"),
        m80_version: "v0.0.0",
    }
}

fn linux_x86_64() -> HostTuple<'static> {
    HostTuple {
        os: "linux",
        arch: "x86_64",
    }
}

fn fetch_request(index_url: &str) -> AssetIndexFetchRequest<'_> {
    AssetIndexFetchRequest {
        index_url,
        release_tag: "v0.0.0",
        host: linux_x86_64(),
        image_kind: Some("minimal"),
    }
}

fn write_index_with_sidecar(root: &Path, json: &str) -> String {
    let index = root.join(ASSET_INDEX_NAME);
    fs::write(&index, json).unwrap();
    fs::write(
        root.join(format!("{ASSET_INDEX_NAME}.sha256")),
        format!("{}  {ASSET_INDEX_NAME}\n", sha256_bytes(json.as_bytes())),
    )
    .unwrap();
    file_url(&index)
}

fn file_url(path: &Path) -> String {
    format!("file://{}", path.display())
}

fn index_json_with_schema(schema_version: u32) -> String {
    index_json(
        "v0.0.0",
        asset_json("linux", "x86_64", "minimal", "v0.0.0", "v0.0.0"),
    )
    .replace(
        "\"schema_version\": 1",
        &format!("\"schema_version\": {schema_version}"),
    )
}

fn index_json_with_assets(assets: String) -> String {
    index_json("v0.0.0", assets)
}

fn parse_index_with_asset_field(
    field: &str,
    replacement: serde_json::Value,
) -> Result<ReleaseAssetIndex, AssetIndexError> {
    let mut value: serde_json::Value = serde_json::from_str(&index_json_with_schema(1)).unwrap();
    value["assets"][0][field] = replacement;

    ReleaseAssetIndex::parse_json(&value.to_string())
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

fn asset_json(
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
