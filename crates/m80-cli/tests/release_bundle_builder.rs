use std::path::PathBuf;
use std::process::Command;

#[test]
fn release_bundle_builder_fixture_suite_passes() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("m80-cli crate should be under crates/")
        .to_path_buf();
    let output = Command::new("python3")
        .arg(repo_root.join("scripts/test-release-bundle.py"))
        .current_dir(&repo_root)
        .output()
        .expect("run release bundle fixture suite");

    assert!(
        output.status.success(),
        "release bundle fixture suite failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
