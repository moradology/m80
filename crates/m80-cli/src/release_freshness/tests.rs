use super::*;

#[test]
fn reader_accepts_freshness_status_artifact_used_by_ci() {
    let metadata = metadata(Some("v1.2.3"), Some("2026-05-21T12:00:00Z"));

    assert_eq!(metadata.repository(), "moradology/m80");
    assert_eq!(metadata.latest_tag(), "v1.2.3");
    assert_eq!(
        metadata.published_at().as_i64(),
        timestamp("2026-05-21T12:00:00Z").as_i64()
    );
    assert_eq!(metadata.safety_floor().minimum_safe_tag(), None);
    assert!(metadata.safety_floor().yanked_tags().is_empty());
    assert_eq!(
        compare_freshness(
            ActiveInstallVersion::Release { tag: "v1.2.3" },
            LatestMetadata::Available(&metadata),
            UnixSeconds::new(timestamp("2026-05-21T12:30:00Z").as_i64()),
        )
        .as_str(),
        "current"
    );
}

#[test]
fn reader_accepts_optional_safety_floor_fields() {
    let metadata = read_freshness_status_artifact_json(&status_artifact_with_safety(
        Some("v1.2.3"),
        Some("2026-05-21T12:00:00Z"),
        Some("v1.2.0"),
        &["v1.1.9"],
    ))
    .expect("safety status artifact should parse");

    assert_eq!(metadata.safety_floor().minimum_safe_tag(), Some("v1.2.0"));
    assert!(metadata.safety_floor().is_yanked("v1.1.9"));
    assert!(!metadata.safety_floor().is_yanked("v1.2.3"));
}

#[test]
fn malformed_safety_floor_tags_fail_closed() {
    let err = read_freshness_status_artifact_json(&status_artifact_with_safety(
        Some("v1.2.3"),
        Some("2026-05-21T12:00:00Z"),
        Some("v1.2.0-rc.1"),
        &[],
    ))
    .expect_err("prerelease safety floor should fail");

    assert_eq!(
        err,
        FreshnessMetadataError::MalformedSafetyTag {
            field: "safety_floor.minimum_safe_tag",
            tag: "v1.2.0-rc.1".to_owned(),
        }
    );
}

#[test]
fn comparison_marks_outdated_release_without_downloading_bundles() {
    let metadata = metadata(Some("v1.2.3"), Some("2026-05-21T12:00:00Z"));

    let state = compare_freshness(
        ActiveInstallVersion::Release { tag: "v1.2.2" },
        LatestMetadata::Available(&metadata),
        timestamp("2026-05-21T12:30:00Z"),
    );

    assert_eq!(state, FreshnessState::Outdated);
}

#[test]
fn missing_latest_tag_fails_closed() {
    let err =
        read_freshness_status_artifact_json(&status_artifact(None, Some("2026-05-21T12:00:00Z")))
            .expect_err("missing latest tag should fail");

    assert_eq!(err, FreshnessMetadataError::MissingLatestTag);
}

#[test]
fn malformed_latest_tag_fails_closed() {
    let err = read_freshness_status_artifact_json(&status_artifact(
        Some("v1.2.3-rc.1"),
        Some("2026-05-21T12:00:00Z"),
    ))
    .expect_err("prerelease latest tag should fail");

    assert_eq!(
        err,
        FreshnessMetadataError::MalformedLatestTag {
            tag: "v1.2.3-rc.1".to_owned()
        }
    );
}

#[test]
fn stale_published_timestamp_is_a_finite_comparison_state() {
    let metadata = metadata(Some("v1.2.3"), Some("2026-05-19T11:59:59Z"));

    let state = compare_freshness(
        ActiveInstallVersion::Release { tag: "v1.2.3" },
        LatestMetadata::Available(&metadata),
        timestamp("2026-05-21T12:00:00Z"),
    );

    assert_eq!(state, FreshnessState::StaleLatestMetadata);
}

#[test]
fn comparison_has_explicit_offline_prerelease_ineligible_and_dev_states() {
    let metadata = metadata(Some("v1.2.3"), Some("2026-05-21T12:00:00Z"));
    let now = timestamp("2026-05-21T12:30:00Z");

    assert_eq!(
        compare_freshness(
            ActiveInstallVersion::Release { tag: "v1.2.3" },
            LatestMetadata::UnknownOffline,
            now,
        ),
        FreshnessState::UnknownOffline
    );
    assert_eq!(
        compare_freshness(
            ActiveInstallVersion::Prerelease { tag: "v1.2.4-rc.1" },
            LatestMetadata::Available(&metadata),
            now,
        ),
        FreshnessState::PrereleaseActive
    );
    assert_eq!(
        compare_freshness(
            ActiveInstallVersion::Ineligible {
                tag: "not-a-release",
            },
            LatestMetadata::Available(&metadata),
            now,
        ),
        FreshnessState::IneligibleActive
    );
    assert_eq!(
        compare_freshness(
            ActiveInstallVersion::LocalDev,
            LatestMetadata::Available(&metadata),
            now,
        ),
        FreshnessState::LocalDevActive
    );
}

#[test]
fn malformed_published_timestamp_fails_closed() {
    let err = read_freshness_status_artifact_json(&status_artifact(
        Some("v1.2.3"),
        Some("2026-05-21 12:00:00"),
    ))
    .expect_err("malformed published_at should fail");

    assert_eq!(
        err,
        FreshnessMetadataError::MalformedPublishedAt {
            value: "2026-05-21 12:00:00".to_owned()
        }
    );
}

fn metadata(tag: Option<&str>, published_at: Option<&str>) -> LatestFreshnessMetadata {
    read_freshness_status_artifact_json(&status_artifact(tag, published_at))
        .expect("status artifact should parse")
}

fn timestamp(input: &str) -> UnixSeconds {
    parse_rfc3339_utc(input).expect("test timestamp should parse")
}

fn status_artifact(tag: Option<&str>, published_at: Option<&str>) -> String {
    status_artifact_with_safety(tag, published_at, None, &[])
}

fn status_artifact_with_safety(
    tag: Option<&str>,
    published_at: Option<&str>,
    minimum_safe_tag: Option<&str>,
    yanked_tags: &[&str],
) -> String {
    let mut fields = vec![
        r#""schema_version":1"#.to_owned(),
        r#""freshness_network_bounded":true"#.to_owned(),
        r#""repository":"moradology/m80""#.to_owned(),
        r#""fetch_policy":{"connect_timeout_seconds":10,"max_time_seconds":120,"retry_count":2,"retry_delay_seconds":1}"#.to_owned(),
        format!(
            r#""checked_urls":[{{"role":"latest-install","url":"{}","release_tag":"latest","asset_name":"install.sh","sources":["release-url-contract:latest-install"],"size_bytes":123,"sha256":"{}"}}]"#,
            crate::release_urls::latest_install_url(),
            "1".repeat(64)
        ),
        format!(
            r#""public_assets":[{{"name":"install.sh","role":"installer","url":"{}","release_tag":"{}","size_bytes":123,"sha256":"{}"}}]"#,
            crate::release_urls::release_install_url(tag.unwrap_or("v1.2.3")),
            tag.unwrap_or("v1.2.3"),
            "2".repeat(64)
        ),
    ];
    if let Some(tag) = tag {
        fields.push(format!(r#""resolved_tag":"{tag}""#));
    }
    if let Some(published_at) = published_at {
        fields.push(format!(r#""published_at":"{published_at}""#));
    }
    if minimum_safe_tag.is_some() || !yanked_tags.is_empty() {
        let minimum = minimum_safe_tag
            .map(|tag| format!(r#""minimum_safe_tag":"{tag}""#))
            .into_iter();
        let yanked = if yanked_tags.is_empty() {
            None
        } else {
            Some(format!(
                r#""yanked_tags":[{}]"#,
                yanked_tags
                    .iter()
                    .map(|tag| format!(r#""{tag}""#))
                    .collect::<Vec<_>>()
                    .join(",")
            ))
        };
        let safety_fields = minimum.chain(yanked).collect::<Vec<_>>().join(",");
        fields.push(format!(r#""safety_floor":{{{safety_fields}}}"#));
    }
    format!("{{{}}}", fields.join(","))
}
