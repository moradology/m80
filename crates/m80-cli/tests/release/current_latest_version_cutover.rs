//! Contract test for the current public-latest repair version cutover.

use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("crate lives under repo/crates/m80-cli")
        .to_path_buf()
}

#[test]
fn current_latest_version_cutover_names_real_repair_tag() {
    let root = repo_root();
    let cargo_toml = fs::read_to_string(root.join("Cargo.toml")).expect("read root Cargo.toml");
    let runbook =
        fs::read_to_string(root.join("docs/runbook/release.md")).expect("read release runbook");
    let behavior =
        fs::read_to_string(root.join("docs/behaviors/release/current-latest-version-cutover.md"))
            .expect("read current latest version cutover behavior doc");
    let preflight_tests =
        fs::read_to_string(root.join("scripts/test-current-latest-repair-preflight.py"))
            .expect("read current latest repair preflight tests");
    let installer_fixture =
        fs::read_to_string(root.join("crates/m80-cli/tests/release/installer_layout/fixture.rs"))
            .expect("read installer layout fixture");

    assert!(
        cargo_toml.contains("version = \"0.2.9\""),
        "workspace package version must match the current repair tag"
    );
    assert!(
        runbook.contains("workspace package version `0.2.9`")
            && runbook.contains("expected release tag is `v0.2.9`"),
        "release runbook must document the real current repair version"
    );
    assert!(
        behavior.contains("matching stable tag is `v0.2.9`")
            && behavior.contains("`v0.2.7` installer handoff"),
        "behavior doc must name the repaired tag and the superseded latest state"
    );
    assert!(
        preflight_tests.contains("test_cli_accepts_current_workspace_version_without_override"),
        "preflight tests must prove v0.2.7 works without a workspace-version override"
    );
    assert!(
        installer_fixture.contains("concat!(\"v\", env!(\"CARGO_PKG_VERSION\"))"),
        "installer layout release fixtures must derive the release tag from the workspace package version"
    );
}
