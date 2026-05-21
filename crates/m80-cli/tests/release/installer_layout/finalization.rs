use std::fs;

use super::fixture::write_release_bundle;
use super::{read_repo_file, run_install, seed_previous_active_install, HostPrereqFixture};

#[test]
fn installed_layout_doc_names_directory_contract_and_tests() {
    let doc = read_repo_file("docs/behaviors/release/installed-layout.md");

    for required in [
        "`<install-root>/versions/<release_tag>`",
        "`bin/m80`",
        "`artifacts/output.ext4.manifest.json`",
        "`artifacts/install-provenance.json`",
        "`artifacts/host-binaries.manifest.json`",
        "`<install-root>/active`",
        "install_bundle_layout_copies_verified_bundle_into_version_dir",
        "install_bundle_layout_downloads_http_bundle_into_version_dir",
        "install_bundle_layout_rejects_remote_bundle_checksum_mismatch_before_extract",
        "install_bundle_layout_rejects_remote_404_before_extract",
        "install_bundle_layout_deletes_truncated_download_partial",
        "install_bundle_layout_rejects_redirect_to_different_fixture_host",
        "install_bundle_layout_rejects_checksum_redirect_to_different_fixture_host",
        "install_bundle_layout_missing_required_bundle_file_fails_before_activation",
        "install_bundle_layout_duplicate_bundle_path_fails_before_activation",
        "install_bundle_layout_permission_failure_leaves_active_state_untouched",
        "install_bundle_layout_manifest_failure_leaves_previous_active_selected",
        "install_bundle_layout_profile_failure_leaves_previous_active_and_profile",
        "install_bundle_layout_injected_interruption_leaves_previous_active_selected",
        "install_bundle_layout_cleans_abandoned_staging_dirs",
        "missing_integrity_predicate_aborts_before_install_root_mutation",
        "missing_asset_index_aborts_before_install_root_mutation",
        "public_sha256s_digest_mismatch_aborts_before_install_root_mutation",
        "tampered_bundle_aborts_before_install_root_mutation",
        "install_bundle_layout_dry_run_never_reads_or_writes_bundle_layout",
    ] {
        assert!(
            doc.contains(required),
            "installed layout doc missing {required:?}"
        );
    }
}

#[test]
fn install_state_doc_names_proof_cache_manifest_contract() {
    let doc = read_repo_file("docs/behaviors/release/install-state.md");

    for required in [
        "`<install-root>/versions/<release_tag>`",
        "`<install-root>/active`",
        "`<install-root>/versions/<release_tag>/artifacts/release-proof-cache/`",
        "`<install-root>/versions/<release_tag>/artifacts/release-proof-cache/manifest.json`",
        "`manifest_digest`",
        "`integrity_predicate`",
        "`attestation_bundle`",
        "`attestation_metadata`",
        "`asset_index`",
        "`public_sha256s`",
        "`checksum_sidecars`",
        "`trust_policy`",
        "`verifier_versions`",
        "`deny_unknown_fields`",
        "complete_manifest_parses_and_validates_digest",
        "missing_required_field_fails_closed",
        "unknown_field_fails_closed",
        "malformed_material_digest_fails_closed",
        "malformed_manifest_digest_fails_closed",
        "write_verified_release_proof_cache_copies_manifest_and_mode_checks_material",
        "write_verified_release_proof_cache_rejects_existing_cache_target_file",
        "proof_cache_write_failure_leaves_previous_active_profile_and_config_selected",
        "proof_cache_manifest_digest_failure_leaves_previous_active_profile_and_config_selected",
        "proof_cache_mode_failure_leaves_previous_active_profile_and_config_selected",
        "before default profile/config writes",
    ] {
        assert!(
            doc.contains(required),
            "install state doc missing {required:?}"
        );
    }
}

#[test]
fn install_bundle_layout_manifest_failure_leaves_previous_active_selected() {
    let bundle = write_release_bundle(None);
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let previous = seed_previous_active_install(&install_root);

    let output = run_install(
        &bundle,
        &install_root,
        None,
        &[("M80_FIRECRACKER_BIN", "/definitely/missing/firecracker")],
        &[],
    );

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("firecracker") || stderr.contains("Firecracker"),
        "unexpected stderr: {stderr}"
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        previous
    );
}

#[test]
fn install_bundle_layout_profile_failure_leaves_previous_active_and_profile() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let previous = seed_previous_active_install(&install_root);
    let profile_path = install_root.join("profiles/default.toml");
    fs::create_dir_all(profile_path.parent().unwrap()).unwrap();
    fs::write(&profile_path, "description = \"old profile\"\n").unwrap();
    fs::write(install_root.join("config.toml"), "surprise = true\n").unwrap();

    let output = run_install(&bundle, &install_root, Some(&host), &[], &[]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("unknown config key"),
        "unexpected stderr: {stderr}"
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        previous
    );
    assert_eq!(
        fs::read_to_string(profile_path).unwrap(),
        "description = \"old profile\"\n"
    );
}

#[test]
fn install_bundle_layout_injected_interruption_leaves_previous_active_selected() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let previous = seed_previous_active_install(&install_root);

    let output = run_install(
        &bundle,
        &install_root,
        Some(&host),
        &[("M80_INSTALL_INJECT_INTERRUPTION_AFTER_PROFILE", "1")],
        &[],
    );

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("injected interruption"),
        "unexpected stderr: {stderr}"
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        previous
    );
}

#[test]
fn install_bundle_layout_cleans_abandoned_staging_dirs() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let stale = install_root.join(".staging/layout-stale");
    fs::create_dir_all(&stale).unwrap();
    fs::write(stale.join("partial"), b"partial bundle").unwrap();

    let output = run_install(&bundle, &install_root, Some(&host), &[], &[]);

    assert!(
        output.status.success(),
        "install failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !stale.exists(),
        "abandoned staging dir should be removed before new staging"
    );
    let staging_parent = install_root.join(".staging");
    assert!(
        fs::read_dir(&staging_parent).unwrap().next().is_none(),
        "staging parent should not keep abandoned layout dirs"
    );
}

#[test]
fn install_finalization_transaction_doc_names_state_machine_and_tests() {
    let doc = read_repo_file("docs/behaviors/release/install-finalization-transaction.md");

    for required in [
        "`m80-o3uh9.16.1`",
        "`bundle_verification`",
        "`host_prerequisite_verification`",
        "`install_provenance`",
        "`release_proof_cache`",
        "`host_binaries_manifest`",
        "`default_profile`",
        "`preflight_smoke_gate`",
        "`active_pointer_flip`",
        "`<install-root>/active` as an absolute symlink",
        "hostless preflight fixture",
        "GitHub release bundle URLs use live",
        "host preflight",
        "install_bundle_layout_manifest_failure_leaves_previous_active_selected",
        "install_bundle_layout_profile_failure_leaves_previous_active_and_profile",
        "install_bundle_layout_injected_interruption_leaves_previous_active_selected",
        "proof_cache_write_failure_leaves_previous_active_profile_and_config_selected",
        "proof_cache_manifest_digest_failure_leaves_previous_active_profile_and_config_selected",
        "proof_cache_mode_failure_leaves_previous_active_profile_and_config_selected",
        "write_verified_release_proof_cache_rejects_existing_cache_target_file",
        "install_bundle_layout_cleans_abandoned_staging_dirs",
    ] {
        assert!(
            doc.contains(required),
            "install finalization doc missing {required:?}"
        );
    }
}
