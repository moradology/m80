use std::fs;
use std::path::PathBuf;

#[test]
fn install_state_doc_names_resolver_states_and_guards() {
    let doc = read_repo_file("docs/behaviors/release/install-state.md");

    for required in [
        "`m80-o3uh9.16.7.2`",
        "`m80-o3uh9.16.7.1`",
        "`m80 install-status`",
        "`m80 --json install-status`",
        "`healthy_active_release`",
        "`missing_active_pointer`",
        "`dangling_active_pointer`",
        "`local_dev_tree`",
        "`stale_profile_target`",
        "`explicit_override`",
        "`missing_install_metadata`",
        "`stale_install_metadata`",
        "`tampered_proof_cache`",
        "`invalid_install_metadata`",
        "`m80-o3uh9.16.7.3`",
        "`m80-o3uh9.16.7.4`",
        "bundle.json",
        "install-provenance.json",
        "host-binaries.manifest.json",
        "release-proof-cache/manifest.json",
        "`schema_version: 1`",
        "`next_action`",
        "`mismatches`",
        "`expected_tag`",
        "`observed_tag`",
        "`explicit_profile_override`",
        "`install_root_override`",
        "`deny_unknown_fields`",
        "`O_NOFOLLOW`",
        "symlinked installed references",
        "does not execute",
        "installed binaries",
        "rejects active-pointer traversal",
        "install-owned profile",
        "paths outside the install root",
        "`M80_DEFAULT_PROFILE`",
        "`--profile`",
    ] {
        assert!(
            doc.contains(required),
            "install-state doc missing {required:?}"
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
