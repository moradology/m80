use std::fs;
use std::path::PathBuf;

#[test]
fn install_state_doc_names_resolver_states_and_guards() {
    let doc = read_repo_file("docs/behaviors/release/install-state.md");

    for required in [
        "`m80-o3uh9.16.7.2`",
        "`healthy_active_release`",
        "`missing_active_pointer`",
        "`dangling_active_pointer`",
        "`local_dev_tree`",
        "`stale_profile_target`",
        "`explicit_override`",
        "`invalid_install_metadata`",
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
