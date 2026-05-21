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
        "`docs/behaviors/release/m80-release-trust-policy.json`",
        "`m80-release-integrity.attestation.jsonl`",
        "`m80-release-attestation.json`",
        "`keyset_id`",
        "`allowed_signers`",
        "`rotation`",
        "`hard-fail-expired`",
        "`gh attestation verify`",
        "`--deny-self-hosted-runners`",
        "`--signer-workflow`",
        "`--source-digest`",
        "`material_class`",
        "`retry_command=m80 install --bundle-url '<url>' --install-root '<path>'`",
        "Install or upgrade GitHub CLI with attestation support",
        "moradology/m80/.github/workflows/release-artifacts.yml",
        "https://token.actions.githubusercontent.com",
        "certificate_not_before",
        "certificate_not_after",
        "`release_tag`",
        "`commit_sha`",
        "M80_RELEASE_COMMIT=\"$(git rev-list -n 1 \"$M80_RELEASE_TAG\")\"",
        "--commit-sha \"$M80_RELEASE_COMMIT\"",
        "`target`",
        "`rust_toolchain`",
        "`m80_package_version`",
        "`bundle_metadata_sha256`",
        "`subjects`",
        "test_release_integrity_material_accepts_complete_public_subject_set",
        "m80-linux-x86_64.tar.gz",
        "install.sh",
        "m80-release-assets.json",
        "m80-bootstrap-selector.tsv",
        "m80-release-build.json",
        "SHA256SUMS",
        "scripts/verify-release-integrity.py",
        "scripts/verify-release-bundle.py",
        "--verify-integrity",
        "test_human_release_dist_verifier_accepts_clean_public_dist",
        "test_human_release_dist_verifier_rejects_tampered_tarball",
        "test_human_release_dist_verifier_rejects_tampered_install_sh",
        "test_human_release_dist_verifier_rejects_wrong_tag",
        "test_human_release_dist_verifier_rejects_missing_attestation",
        "test_human_release_dist_verifier_rejects_missing_sidecar",
        "test_release_integrity_material_rejects_wrong_tag",
        "test_release_integrity_material_rejects_missing_install_subject",
        "test_release_integrity_material_rejects_missing_asset_index_subject",
        "test_release_integrity_material_rejects_missing_bootstrap_selector_subject",
        "test_release_integrity_material_rejects_missing_build_manifest_subject",
        "test_release_integrity_material_rejects_build_manifest_commit_mismatch",
        "test_release_integrity_material_rejects_build_manifest_malformed_container_digest",
        "test_release_integrity_material_rejects_unexpected_extra_subject",
        "test_release_integrity_material_rejects_subject_digest_mismatch",
        "test_release_integrity_material_rejects_tampered_install_hash",
        "test_release_integrity_material_rejects_signed_bootstrap_selector_drift",
        "test_release_integrity_material_rejects_signed_bootstrap_selector_shell_metacharacters",
        "test_release_integrity_material_rejects_unsupported_verifier_version",
        "test_release_integrity_material_preflights_missing_verifier_before_material_read",
        "test_release_attestation_metadata_writer_preflights_missing_verifier_before_material_read",
        "test_release_integrity_material_rejects_too_old_attestation_verifier",
        "test_release_integrity_material_rejects_attestation_verifier_missing_required_flag",
        "test_release_integrity_material_rejects_missing_attestation_metadata",
        "test_release_integrity_material_rejects_missing_trust_policy",
        "test_release_integrity_material_rejects_missing_attestation_bundle",
        "test_release_integrity_material_rejects_unsupported_trust_policy_schema",
        "test_release_integrity_material_rejects_unsupported_attestation_metadata_schema",
        "test_release_integrity_material_rejects_trust_policy_mechanism_mismatch",
        "test_release_integrity_material_rejects_failed_cryptographic_attestation",
        "test_release_integrity_material_rejects_attestation_without_material_subject",
        "test_release_integrity_material_rejects_attestation_subject_digest_mismatch",
        "test_release_integrity_material_rejects_attestation_subject_name_mismatch",
        "test_release_integrity_material_rejects_unknown_signer",
        "test_release_integrity_material_rejects_stale_keyset",
        "test_release_integrity_material_rejects_expired_certificate_window",
        "test_release_integrity_material_rejects_expired_trust_policy",
        "test_release_integrity_material_rejects_boolean_rotation_overlap",
        "test_release_integrity_material_rejects_replayed_tag_attestation",
        "test_release_integrity_material_rejects_replayed_repo_attestation",
        "test_rendered_install_script_rejects_build_manifest_commit_mismatch_before_bundle",
        "test_rendered_install_script_rejects_build_manifest_target_triples_mismatch_before_bundle",
        "test_rendered_install_script_rejects_build_manifest_rust_toolchain_mismatch_before_bundle",
        "test_rendered_install_script_rejects_build_manifest_target_mismatch_before_bundle",
        "test_rendered_install_script_rejects_build_manifest_package_version_mismatch_before_bundle",
        "test_rendered_install_script_rejects_build_manifest_metadata_hash_mismatch_before_bundle",
        "test_rendered_install_script_rejects_build_manifest_without_builder_material_before_bundle",
        "test_rendered_install_script_rejects_build_manifest_malformed_container_digest_before_bundle",
        "official_release_verifier_failure_matrix_leaves_install_root_unchanged",
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
    assert!(runbook.contains("python3 scripts/verify-release-bundle.py"));
    assert!(runbook.contains("/tmp/m80-release-dist/m80-linux-x86_64.tar.gz"));
    assert!(runbook.contains("--verify-integrity"));
    assert!(runbook.contains("python3 scripts/verify-release-integrity.py"));
    assert!(runbook.contains("M80_RELEASE_COMMIT=\"$(git rev-list -n 1 \"$M80_RELEASE_TAG\")\""));
    assert!(runbook.contains("--commit-sha \"$M80_RELEASE_COMMIT\""));
    assert!(runbook.contains("--trust-policy docs/behaviors/release/m80-release-trust-policy.json"));
    assert!(runbook.contains(
        "--attestation-bundle /tmp/m80-release-dist/m80-release-integrity.attestation.jsonl"
    ));
    assert!(runbook
        .contains("--attestation-metadata /tmp/m80-release-dist/m80-release-attestation.json"));
    assert!(runbook.contains("--verification-time \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\""));
    assert!(runbook.contains("gh attestation"));
    assert!(runbook.contains("--source-digest"));
    assert!(runbook.contains("Install or upgrade GitHub CLI with attestation support"));
    assert!(runbook.contains("share one trust-anchor path"));
    assert!(runbook.contains("does not require root"));
}

#[test]
fn release_bundle_builder_doc_names_proof_material_outputs() {
    let doc = read_repo_file("docs/behaviors/release/bundle-builder.md");

    for required in [
        "`--commit-sha`",
        "`--rust-toolchain`",
        "`--target-triple`",
        "`--builder-identity`",
        "`--builder-os-image`",
        "`--apt-package-version`",
        "m80-release-build.json",
        "m80-release-integrity.json",
        "m80-release-integrity.attestation.jsonl",
        "m80-release-attestation.json",
        "tag workflow",
    ] {
        assert!(
            doc.contains(required),
            "bundle-builder doc missing {required:?}"
        );
    }
}

#[test]
fn release_subject_completeness_docs_name_public_asset_set() {
    let bundle_contract = read_repo_file("docs/behaviors/release/bundle-contract.md");
    let runbook = read_repo_file("docs/runbook/release.md");

    for doc in [&bundle_contract, &runbook] {
        assert!(doc.contains("complete"));
        assert!(doc.contains("public"));
        for asset in [
            "m80-linux-x86_64.tar.gz",
            "m80-linux-x86_64.tar.gz.sha256",
            "install.sh",
            "install.sh.sha256",
            "m80-linux-x86_64.bundle.json",
            "m80-linux-x86_64.bundle.json.sha256",
            "m80-release-assets.json",
            "m80-release-assets.json.sha256",
            "m80-bootstrap-selector.tsv",
            "m80-bootstrap-selector.tsv.sha256",
            "m80-release-build.json",
            "m80-release-build.json.sha256",
            "SHA256SUMS",
        ] {
            assert!(doc.contains(asset), "subject-set doc missing {asset}");
        }
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
