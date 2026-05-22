use std::fs;
use std::path::PathBuf;

#[test]
fn current_latest_repair_release_note_names_supersession_and_public_evidence() {
    let note = read_repo_file("docs/behaviors/release/current-latest-repair-release-note.md");
    let readme = read_repo_file("README.md");
    let runbook = read_repo_file("docs/runbook/release.md");

    for required in [
        "v0.2.7",
        "388d2d5aa13418e22c1144963b0f66dd3588a92b",
        "https://github.com/moradology/m80/releases/latest",
        "v0.2.6` was selected by GitHub\nlatest but did not publish `install.sh`",
        "release-readiness-public-access.json",
        "current-latest-repair-publish-proof.json",
        "m80-release-publish-decision-26263525140",
        "m80-release-public-access-26263525140",
        "Workflow-only receipts",
        "env -u GH_TOKEN -u GITHUB_TOKEN",
    ] {
        assert!(note.contains(required), "release note missing {required:?}");
    }

    for forbidden in ["/tmp/m80-", "file://", "localhost", "compatibility matrix"] {
        assert!(
            !note.contains(forbidden),
            "release note leaked local or compatibility-matrix text: {forbidden}"
        );
    }

    assert!(
        !readme.contains("v0.2.6") && !readme.contains("compatibility matrix"),
        "README quickstart must not grow old-release compatibility archaeology"
    );
    assert!(
        !runbook.contains("compatibility matrix"),
        "release runbook must keep the normal path short"
    );
}

fn read_repo_file(path: &str) -> String {
    fs::read_to_string(repo_root().join(path)).unwrap_or_else(|error| {
        panic!("failed to read repo file {path}: {error}");
    })
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("crate lives under repo/crates/m80-cli")
        .to_path_buf()
}
