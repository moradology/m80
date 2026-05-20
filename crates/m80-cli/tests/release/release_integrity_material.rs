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
        "moradology/m80/.github/workflows/release-artifacts.yml",
        "https://token.actions.githubusercontent.com",
        "certificate_not_before",
        "certificate_not_after",
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
    assert!(runbook.contains("--trust-policy docs/behaviors/release/m80-release-trust-policy.json"));
    assert!(runbook.contains(
        "--attestation-bundle /tmp/m80-release-dist/m80-release-integrity.attestation.jsonl"
    ));
    assert!(runbook
        .contains("--attestation-metadata /tmp/m80-release-dist/m80-release-attestation.json"));
    assert!(runbook.contains("--verification-time \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\""));
    assert!(runbook.contains("gh attestation"));
    assert!(runbook.contains("share one trust-anchor path"));
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
