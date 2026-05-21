use std::fs;
use std::path::PathBuf;

#[test]
fn direct_url_fixture_harness_doc_names_network_and_matrix_contract() {
    let doc = read_repo_file("docs/behaviors/release/direct-url-fixture-harness.md");

    for required in [
        "`write_direct_release_materials_with`",
        "network-free release fixture",
        "shadows `curl` through `PATH`",
        "`M80_FAKE_CURL_LOG`",
        "did not fetch the bundle before required pre-bundle material passed",
        "SHA-256 digests",
        "`install.sh`",
        "`SHA256SUMS`",
        "release-integrity predicate",
        "release-attestation metadata",
        "complete official release material",
        "missing material",
        "mismatched repository or release tag",
        "tampered bundle bytes",
        "stale asset-index rows",
        "stale checksum sidecars",
        "missing `install.sh` digest rows",
        "stale public `SHA256SUMS` rows",
        "bad release-integrity predicate subjects",
        "failed or malformed GitHub attestation output",
        "`fixture_omit_matrix_covers_every_direct_url_material_class`",
        "Adding a new required verifier material class must add a fixture case",
    ] {
        assert!(
            doc.contains(required),
            "direct URL fixture harness doc missing {required:?}"
        );
    }
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
