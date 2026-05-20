use std::fs;
use std::path::PathBuf;

#[test]
fn install_handoff_identity_doc_names_contract_and_regressions() {
    let doc = read_repo_file("docs/behaviors/release/install-handoff-identity.md");

    for required in [
        "`bin/m80`",
        "`bin/m80 --json version`",
        "`version_status` is `release`",
        "`source_commit` equals the signed release-integrity commit",
        "`target` equals the bundle metadata and build manifest target",
        "`target_triple` is recorded in the build manifest target triples",
        "`protocol_version` equals both bundle metadata protocol fields",
        "stops before `m80 install --bundle-url ...` runs",
        "test_rendered_install_script_rejects_extracted_m80_source_commit_mismatch_before_install",
        "test_rendered_install_script_rejects_extracted_m80_release_tag_mismatch_before_install",
        "test_rendered_install_script_rejects_extracted_m80_target_mismatch_before_install",
        "test_rendered_install_script_rejects_extracted_m80_target_triple_mismatch_before_install",
        "test_rendered_install_script_rejects_extracted_m80_dev_identity_before_install",
        "test_rendered_install_script_rejects_extracted_m80_malformed_identity_before_install",
        "test_rendered_install_script_rejects_extracted_m80_missing_identity_field_before_install",
        "test_rejects_binary_source_commit_mismatch",
        "test_rejects_binary_target_mismatch",
        "test_rejects_binary_target_triple_mismatch",
    ] {
        assert!(
            doc.contains(required),
            "install handoff identity doc missing {required:?}"
        );
    }
}

#[test]
fn installer_template_verifies_extracted_binary_before_delegation() {
    let install_sh = read_repo_file("scripts/install.sh");
    let verify_call = install_sh
        .find("verify_extracted_m80_identity \"$extract_dir/bin/m80\"")
        .expect("install.sh must verify extracted m80 identity");
    let delegation = install_sh
        .find("\"$extract_dir/bin/m80\" install --bundle-url \"file://$bundle_path\" \"$@\"")
        .expect("install.sh must delegate to extracted m80 install");

    assert!(
        verify_call < delegation,
        "install.sh must verify extracted m80 identity before m80 install delegation"
    );
    assert!(install_sh.contains("\"$binary_path\" --json version > \"$identity_path\""));
    assert!(install_sh.contains("extracted m80 source_commit"));
    assert!(install_sh.contains("verified handoff binary="));
}

#[test]
fn release_runbook_mentions_handoff_identity_check() {
    let runbook = read_repo_file("docs/runbook/release.md");

    assert!(runbook.contains("source commit, and Rust target"));
    assert!(runbook.contains("`source_commit`"));
    assert!(runbook.contains("checks that `m80 --json version` agrees"));
}

#[test]
fn python_release_suite_covers_handoff_identity_failures() {
    let suite = read_repo_file("scripts/test-release-bundle.py");

    for required in [
        "test_rendered_install_script_rejects_extracted_m80_source_commit_mismatch_before_install",
        "test_rendered_install_script_rejects_extracted_m80_release_tag_mismatch_before_install",
        "test_rendered_install_script_rejects_extracted_m80_target_mismatch_before_install",
        "test_rendered_install_script_rejects_extracted_m80_target_triple_mismatch_before_install",
        "test_rendered_install_script_rejects_extracted_m80_dev_identity_before_install",
        "test_rendered_install_script_rejects_extracted_m80_malformed_identity_before_install",
        "test_rendered_install_script_rejects_extracted_m80_missing_identity_field_before_install",
        "test_rejects_binary_source_commit_mismatch",
        "test_rejects_binary_target_mismatch",
        "test_rejects_binary_target_triple_mismatch",
        "replace_bundle_m80_for_install",
    ] {
        assert!(
            suite.contains(required),
            "release bundle Python suite missing {required:?}"
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
