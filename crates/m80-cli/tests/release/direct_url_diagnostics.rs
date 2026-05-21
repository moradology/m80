use std::fs;
use std::path::PathBuf;

#[test]
fn direct_url_diagnostics_doc_names_stable_contract() {
    let doc = read_repo_file("docs/behaviors/release/direct-url-diagnostics.md");
    for required in [
        "`m80 install --bundle-url <URL>` is an explicit development/operator override",
        "shown in the README",
        "quickstart, whose generated freshness status",
        "generated freshness status controls whether the latest",
        "installer is currently public-proven",
        "resolved release tag",
        "bundle asset",
        "`install.sh` SHA-256",
        "public `SHA256SUMS` SHA-256",
        "asset-index SHA-256",
        "proof-cache destination",
        "`proof_cache_written=true`",
        "Local fixture/operator bundle installs do not write",
        "release proof cache",
        "`retry_command=m80 install --bundle-url '<url>' --install-root '<path>'`",
        "`direct_url_classifier`",
        "`release_material_fetch`",
        "`release_material_digest`",
        "`release_material_attestation`",
        "`release_material_stale`",
        "`install_no_write_rollback`",
        "json_envelope_codes_direct_url_diagnostics",
    ] {
        assert!(
            doc.contains(required),
            "direct URL diagnostics doc missing {required:?}"
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
