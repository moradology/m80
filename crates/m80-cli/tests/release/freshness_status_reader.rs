use std::fs;
use std::path::PathBuf;

#[test]
fn freshness_status_reader_doc_names_artifact_source_and_states() {
    let doc = read_repo_file("docs/behaviors/release/freshness-status-reader.md");

    for required in [
        "`m80-o3uh9.16.9.1`",
        "`m80-latest-freshness-proof.json`",
        "`.github/workflows/latest-freshness.yml`",
        "python3 scripts/release_freshness.py --docs-root . --json --proof-out \"$FRESHNESS_PROOF\"",
        "`freshness_network_bounded: true`",
        "`repository: \"moradology/m80\"`",
        "`resolved_tag`",
        "`published_at`",
        "`current`",
        "`outdated`",
        "`unknown_offline`",
        "`stale_latest_metadata`",
        "`prerelease_active`",
        "`ineligible_active`",
        "`local_dev_active`",
        "does not download release bundles",
        "mutate install-root state",
        "Malformed status metadata fails closed",
    ] {
        assert!(
            doc.contains(required),
            "freshness status reader doc missing {required:?}"
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
