use std::fs;
use std::path::PathBuf;

#[test]
fn downgrade_refusal_doc_names_policy_and_json_contract() {
    let doc = read_repo_file("docs/behaviors/release/downgrade-refusal.md");

    for required in [
        "`m80-o3uh9.16.12.3`",
        "`downgrade_refused`",
        "`ReleaseTransition`",
        "`active_tag`",
        "`requested_tag`",
        "`observed_ordering=target_older`",
        "`rollback_command`",
        "`release_tag_source_refuses_downgrade_before_index_fetch`",
        "`official_bundle_url_refuses_downgrade_before_attestation_preflight`",
        "`missing_active_metadata_still_refuses_older_target_by_pointer_tag`",
    ] {
        assert!(doc.contains(required), "doc missing {required:?}");
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
