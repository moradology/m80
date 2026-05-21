use std::fs;
use std::path::PathBuf;

#[test]
fn current_latest_repair_preflight_doc_names_guard_and_ci_contract() {
    let doc = read_repo_file("docs/behaviors/release/current-latest-repair-preflight.md");
    let workflow = read_repo_file(".github/workflows/release-artifacts.yml");
    let ci = read_repo_file(".github/workflows/ci.yml");

    for required in [
        "`m80-o3uh9.21.7.1.1`",
        "`scripts/current_latest_repair_preflight.py`",
        "`target_tag`",
        "`source_commit`",
        "`workspace_package_version`",
        "`expected_release_tag`",
        "`dirty_tree`",
        "`existing_latest`",
        "`release_order`",
        "`supersedes_missing_installer_latest_state`",
        "`v0.2.6`",
        "fails closed",
        "packaging",
        "unknown release state",
    ] {
        assert!(
            doc.contains(required),
            "current latest repair preflight doc missing {required:?}"
        );
    }

    assert!(
        workflow.contains("scripts/current_latest_repair_preflight.py"),
        "release workflow must run current latest repair preflight"
    );
    assert!(
        workflow.find("scripts/current_latest_repair_preflight.py")
            < workflow.find("scripts/package-release-bundle.py"),
        "preflight must run before release packaging"
    );
    assert!(
        workflow.contains("m80-current-latest-repair-preflight-${{ github.run_id }}"),
        "release workflow must upload the preflight artifact"
    );
    assert!(
        ci.contains("python3 scripts/test-current-latest-repair-preflight.py"),
        "CI must run current latest repair preflight tests"
    );
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
