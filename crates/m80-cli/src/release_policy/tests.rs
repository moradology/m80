use super::*;

#[test]
fn stable_tags_have_numeric_ordering() {
    let active = classify_release_tag("v1.2.3");

    assert_eq!(
        release_transition(active.clone(), classify_release_tag("v1.2.4")).state,
        ReleaseTransitionState::UpgradeAllowed
    );
    assert_eq!(
        release_transition(active.clone(), classify_release_tag("v1.2.3")).state,
        ReleaseTransitionState::AlreadyCurrent
    );
    assert_eq!(
        release_transition(active, classify_release_tag("v1.2.2")).state,
        ReleaseTransitionState::DowngradeRefused
    );
    assert_eq!(
        release_transition(
            classify_release_tag("v1.2.99"),
            classify_release_tag("v1.10.0")
        )
        .state,
        ReleaseTransitionState::UpgradeAllowed
    );
}

#[test]
fn nonstable_active_and_target_identities_are_explicit() {
    let target = classify_release_tag("v1.2.4");

    assert_eq!(
        release_transition(classify_release_tag("v1.2.3-rc.1"), target.clone()).state,
        ReleaseTransitionState::ActivePrerelease
    );
    assert_eq!(
        release_transition(classify_release_tag("v1.2.3+build.1"), target.clone()).state,
        ReleaseTransitionState::ActiveBuildMetadata
    );
    assert_eq!(
        release_transition(classify_release_tag("not-a-tag"), target.clone()).state,
        ReleaseTransitionState::ActiveMalformed
    );
    assert_eq!(
        release_transition(local_dev_release_identity(), target).state,
        ReleaseTransitionState::ActiveLocalDev
    );
}

#[test]
fn nonstable_targets_are_explicit() {
    let active = classify_release_tag("v1.2.3");

    assert_eq!(
        release_transition(active.clone(), classify_release_tag("v1.2.4-rc.1")).state,
        ReleaseTransitionState::TargetPrerelease
    );
    assert_eq!(
        release_transition(active.clone(), classify_release_tag("v1.2.4+build.1")).state,
        ReleaseTransitionState::TargetBuildMetadata
    );
    assert_eq!(
        release_transition(active, classify_release_tag("latest")).state,
        ReleaseTransitionState::TargetMalformed
    );
}

#[test]
fn malformed_tag_reasons_are_finite() {
    assert_eq!(
        parse_stable_release_tag("1.2.3").expect_err("missing v prefix should fail"),
        ReleaseTagError::MissingPrefix
    );
    assert_eq!(
        parse_stable_release_tag("v1.2").expect_err("wrong part count should fail"),
        ReleaseTagError::WrongPartCount
    );
    assert_eq!(
        parse_stable_release_tag("v1..3").expect_err("empty part should fail"),
        ReleaseTagError::EmptyPart { index: 1 }
    );
    assert_eq!(
        parse_stable_release_tag("v1.x.3").expect_err("non-digit part should fail"),
        ReleaseTagError::NonDigitPart { index: 1 }
    );
    assert_eq!(
        parse_stable_release_tag("v1.2.3-rc.1").expect_err("prerelease tag should fail"),
        ReleaseTagError::Prerelease
    );
    assert_eq!(
        parse_stable_release_tag("v1.2.3+build.1").expect_err("build metadata tag should fail"),
        ReleaseTagError::BuildMetadata
    );
}

#[test]
fn diagnostics_name_expected_ordering_and_observed_tags() {
    let report = release_transition(
        classify_release_tag("v1.2.3"),
        classify_release_tag("v1.2.2"),
    );

    assert_eq!(report.state.as_str(), "downgrade_refused");
    assert_eq!(report.active_tag.as_deref(), Some("v1.2.3"));
    assert_eq!(report.target_tag.as_deref(), Some("v1.2.2"));
    assert_eq!(report.observed_ordering, "target_older");
    assert!(report.expected_ordering.contains("target stable"));
    assert!(report.diagnostic.contains("active=v1.2.3"));
    assert!(report.diagnostic.contains("target=v1.2.2"));
    assert!(report.diagnostic.contains("ordering=target_older"));
}
