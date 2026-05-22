use std::fs;

#[test]
fn safety_floor_schema_doc_names_version_owner_and_required_fields() {
    let doc = fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/behaviors/release/freshness-status.md"),
    )
    .unwrap();

    for required in [
        "safety_floor.schema_version` is currently `1`",
        "The release freshness publisher",
        "owns this object",
        "\"minimum_safe_tag\": null",
        "\"yanked_releases\": []",
        "advisory release metadata by default",
        "release-blocking only when an explicit release policy or CI gate",
        "does not carry a release-blocking boolean",
        "clients must not infer blocking\nsemantics from the presence of a floor or yanked row",
        "[`freshness-failure-policy.md`](freshness-failure-policy.md)",
        "The freshness verifier always blocks malformed or contradictory",
        "`advisory_url`",
        "`issue_id`",
        "`replacement_command`",
        "`no_replacement_reason`",
        "pinned replacement command",
    ] {
        assert!(
            doc.contains(required),
            "missing safety floor doc text: {required}"
        );
    }
}
