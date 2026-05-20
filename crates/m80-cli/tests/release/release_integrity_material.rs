use std::fs;
use std::path::PathBuf;

#[test]
fn release_integrity_material_doc_names_schema_and_failure_contract() {
    let doc = read_repo_file("docs/behaviors/release/release-integrity-material.md");

    for required in [
        "`m80-release-integrity.json`",
        "`schema_version: 1`",
        "`mechanism: \"github-artifact-attestation\"`",
        "`repository: \"moradology/m80\"`",
        "`release_tag`",
        "`commit_sha`",
        "`target`",
        "`rust_toolchain`",
        "`m80_package_version`",
        "`bundle_metadata_sha256`",
        "`subjects`",
        "m80-linux-x86_64.tar.gz",
        "install.sh",
        "m80-release-assets.json",
        "SHA256SUMS",
        "scripts/verify-release-integrity.py",
        "test_release_integrity_material_rejects_wrong_tag",
        "test_release_integrity_material_rejects_tampered_install_hash",
        "test_release_integrity_material_rejects_unsupported_verifier_version",
    ] {
        assert!(
            doc.contains(required),
            "release integrity material doc missing {required:?}"
        );
    }
}

#[test]
fn release_runbook_includes_human_integrity_verification_command() {
    let runbook = read_repo_file("docs/runbook/release.md");

    assert!(runbook.contains("## Release Integrity Material"));
    assert!(runbook.contains("GitHub Artifact Attestations"));
    assert!(runbook.contains("python3 scripts/verify-release-integrity.py"));
    assert!(runbook.contains("--commit-sha \"$GITHUB_SHA\""));
    assert!(runbook.contains("does not require root"));
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
