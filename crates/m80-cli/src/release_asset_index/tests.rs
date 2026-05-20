use super::*;

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

    assert_eq!(err, AssetIndexError::DevBuildSelection);
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
