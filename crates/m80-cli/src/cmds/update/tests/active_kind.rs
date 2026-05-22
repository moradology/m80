use super::*;

#[test]
fn active_kind_separates_stable_prerelease_ineligible_and_local_dev() {
    for (tag, active_kind, state) in [
        (
            "v1.2.3",
            ActiveInstallKind::StableRelease,
            UpdateCheckState::Current,
        ),
        (
            "v1.2.3-rc.1",
            ActiveInstallKind::Prerelease,
            UpdateCheckState::PrereleaseActive,
        ),
        (
            "v1.2.3+build.1",
            ActiveInstallKind::Ineligible,
            UpdateCheckState::IneligibleActive,
        ),
        (
            "v1.2.x",
            ActiveInstallKind::Ineligible,
            UpdateCheckState::IneligibleActive,
        ),
    ] {
        let output = check_output(
            &active_report(tag),
            latest("v1.2.3", None, &[]),
            UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
        );

        assert_eq!(output.active_kind, active_kind, "active tag {tag}");
        assert_eq!(output.state, state, "active tag {tag}");
        assert_eq!(output.freshness_state, output.state, "active tag {tag}");
    }

    let mut local_dev = active_report("v1.2.3");
    local_dev.state = InstallStateKind::LocalDevTree;
    local_dev.active_pointer.release_tag = None;
    local_dev.metadata = None;
    let output = check_output(
        &local_dev,
        latest("v1.2.3", None, &[]),
        UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
    );

    assert_eq!(output.active_kind, ActiveInstallKind::LocalDev);
    assert_eq!(output.state, UpdateCheckState::LocalDevInstall);
}

#[test]
fn active_kind_separates_missing_and_stale_active_metadata() {
    let mut missing = active_report("v1.2.3");
    missing.state = InstallStateKind::MissingActivePointer;
    missing.active_pointer.release_tag = None;
    let output = check_output(
        &missing,
        latest("v1.2.3", None, &[]),
        UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
    );

    assert_eq!(output.active_kind, ActiveInstallKind::MissingActive);
    assert_eq!(output.state, UpdateCheckState::InstallUnhealthy);

    let mut stale = active_report("v1.2.3");
    stale.state = InstallStateKind::StaleInstallMetadata;
    let output = check_output(
        &stale,
        latest("v1.2.3", None, &[]),
        UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
    );

    assert_eq!(output.active_kind, ActiveInstallKind::StaleActiveMetadata);
    assert_eq!(output.state, UpdateCheckState::InstallUnhealthy);
}

#[test]
fn active_kind_marks_explicit_override_as_ineligible_for_auto_update() {
    let mut report = active_report("v1.2.3");
    report.state = InstallStateKind::ExplicitOverride;

    let output = check_output(
        &report,
        latest("v1.2.3", None, &[]),
        UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
    );

    assert_eq!(output.active_kind, ActiveInstallKind::Ineligible);
    assert_eq!(output.state, UpdateCheckState::InstallUnhealthy);
}

#[test]
fn json_and_human_output_report_active_kind_and_freshness_state() {
    let output = check_output(
        &active_report("v1.2.3-rc.1"),
        latest("v1.2.4", None, &[]),
        UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
    );

    let json: serde_json::Value =
        serde_json::from_str(&crate::json::to_pretty(&output)).expect("update JSON parses");
    assert_eq!(json["data"]["active_kind"], "prerelease");
    assert_eq!(json["data"]["freshness_state"], "prerelease_active");

    let human = render_human(&output);
    assert!(human.contains("active_kind=prerelease\n"));
    assert!(human.contains("freshness_state=prerelease_active\n"));
}
