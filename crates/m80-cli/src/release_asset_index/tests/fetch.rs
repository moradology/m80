use std::fs;
use std::path::Path;

use super::super::fetch::{
    fetch_verified_asset_index, github_release_asset_index_url, sha256_bytes, AssetIndexFetchError,
    AssetIndexFetchRequest,
};
use super::super::ASSET_INDEX_NAME;
use super::{asset_json, index_json, index_json_with_schema, linux_x86_64};

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
