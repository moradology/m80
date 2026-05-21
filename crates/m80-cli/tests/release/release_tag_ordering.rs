use std::fs;
use std::path::Path;

fn read_repo_file(relative: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate should live under workspace root");
    fs::read_to_string(root.join(relative)).expect("repo file should be readable")
}

#[test]
fn release_tag_ordering_doc_names_policy_states_and_consumers() {
    let doc = read_repo_file("docs/behaviors/release/release-tag-ordering.md");
    for required in [
        "`m80-o3uh9.16.12.1`",
        "`vMAJOR.MINOR.PATCH`",
        "`upgrade_allowed`",
        "`already_current`",
        "`downgrade_refused`",
        "`active_prerelease`",
        "`target_prerelease`",
        "`active_build_metadata`",
        "`target_build_metadata`",
        "`active_malformed`",
        "`target_malformed`",
        "`active_local_dev`",
        "`m80 update --check`",
        "`m80 install --release-tag`",
    ] {
        assert!(
            doc.contains(required),
            "release ordering doc missing {required:?}"
        );
    }
}
