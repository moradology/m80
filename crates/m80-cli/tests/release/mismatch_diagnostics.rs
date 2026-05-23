#[test]
fn mismatch_diagnostics_doc_names_fields_and_regressions() {
    let doc_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("docs/behaviors/release/mismatch-diagnostics.md");
    let doc = std::fs::read_to_string(&doc_path).expect("read mismatch diagnostics behavior doc");

    for required in [
        "expected_schema",
        "actual_schema",
        "manifest_path",
        "running_m80_version",
        "selected_install_profile",
        "expected_protocol",
        "actual_guest_protocol",
        "guestd_identity",
        "rootfs_identity",
        "binary_version",
        "bundle_version",
        "repair",
        "stale_manifest_schema_diagnostic_names_profile_path_versions_and_repair",
        "protocol_mismatch_names_expected_actual_guest_rootfs_and_repair",
        "release_tag_source_rejects_binary_tag_mismatch",
        "bundle_url_rejects_release_binary_tag_mismatch",
    ] {
        assert!(
            doc.contains(required),
            "mismatch diagnostics doc missing {required:?}"
        );
    }
}
