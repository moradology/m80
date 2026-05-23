use std::fs;
use std::path::PathBuf;

#[test]
fn asset_index_behavior_doc_names_schema_and_selection_contract() {
    let doc = read_repo_file("docs/behaviors/release/asset-index.md");

    for required in [
        "`schema_version: 1`",
        "`release_tag`",
        "`os`",
        "`arch`",
        "`image_kind`",
        "`guest_protocol_version`",
        "`manifest_schema_version`",
        "`expected_firecracker_version`",
        "`sha256`",
        "`metadata_sha256`",
        "`signature_name`",
        "`attestation_name`",
        "`m80-release-assets.json`",
        "`m80-release-assets.json.sha256`",
        "`m80-bootstrap-selector.tsv`",
        "`m80-release-build.json`",
        "`SHA256SUMS`",
        "shell tokens",
        "shell-unsafe tokens",
        "verify the index sha256",
        "`file://` fixture indexes",
        "expected sha256",
        "observed sha256",
        "fail before bundle download",
        "not a substitute for signed release integrity",
        "re-download",
        "non-SHA-256",
        "zero size/schema/protocol",
        "duplicate",
        "dev builds",
        "`--bundle-url`",
        "pinned release `install.sh` URL",
        "`M80_RELEASE_TAG`",
        "`M80_RELEASE_COMMIT`",
        "`M80_INTERNAL_RELEASE_FIXTURE_ASSET_INDEX_URL`",
        "`scripts/test-release-identity-cli-fixture.py`",
        "`unsupported_host_tuple`",
        "`missing_image_kind`",
        "`stale_asset_index`",
        "`duplicate_default_bundle`",
        "`binary_tag_mismatch`",
    ] {
        assert!(
            doc.contains(required),
            "asset-index behavior doc missing {required:?}"
        );
    }
}

#[test]
fn release_runbook_explains_adding_architectures_without_readme_changes() {
    let runbook = read_repo_file("docs/runbook/release.md");

    assert!(runbook.contains("## Asset Index"));
    assert!(runbook.contains("OS"));
    assert!(runbook.contains("architecture"));
    assert!(runbook.contains("image kind"));
    assert!(runbook.contains("without changing README commands"));
}

fn read_repo_file(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative)).expect("read repository file")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("m80-cli crate should be under crates/")
        .to_path_buf()
}
