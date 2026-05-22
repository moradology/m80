use super::super::{check_output, render_human, LatestStatusInput, UpdateCheckState};
use super::*;
use crate::release_freshness::read_freshness_status_artifact_json;

#[test]
fn safety_fixture_matrix_reports_json_state_and_human_next_action() {
    // Keep the matrix compact and explicit: these are the installed-release
    // states an operator sees before deciding whether to run next_command.
    //
    // Scenario meanings:
    // - current-empty-floor: active == latest and the safety floor has no min/yanks.
    // - outdated-safe-floor: active is old but still above the published floor.
    // - active-below-minimum: active predates the floor and must repair to it.
    // - active-yanked: active is explicitly yanked and must repair to the floor.
    // - prerelease-active: active is not a stable release and has no safe auto-apply.
    // - ineligible-active: active is not parseable as a release and has no auto-apply.
    for case in [
        MatrixCase {
            name: "current-empty-floor",
            active_tag: "v1.2.3",
            latest_tag: "v1.2.3",
            minimum_safe_tag: None,
            yanked_tags: &[],
            expected_state: UpdateCheckState::Current,
            expected_json_state: "current",
            expected_safety_state: "unknown",
            expected_next_command: Some("m80 run -- echo hello"),
            expected_replacement_command: None,
        },
        MatrixCase {
            name: "outdated-safe-floor",
            active_tag: "v1.2.3",
            latest_tag: "v1.2.4",
            minimum_safe_tag: Some("v1.2.0"),
            yanked_tags: &[],
            expected_state: UpdateCheckState::Outdated,
            expected_json_state: "outdated",
            expected_safety_state: "safe",
            expected_next_command: Some(
                "curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.4/install.sh | sudo sh",
            ),
            expected_replacement_command: None,
        },
        MatrixCase {
            name: "active-below-minimum",
            active_tag: "v1.1.9",
            latest_tag: "v1.2.4",
            minimum_safe_tag: Some("v1.2.0"),
            yanked_tags: &[],
            expected_state: UpdateCheckState::Unsafe,
            expected_json_state: "unsafe",
            expected_safety_state: "active_below_minimum",
            expected_next_command: Some(
                "curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.0/install.sh | sudo sh",
            ),
            expected_replacement_command: Some(
                "curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.0/install.sh | sudo sh",
            ),
        },
        MatrixCase {
            name: "active-yanked",
            active_tag: "v1.2.3",
            latest_tag: "v1.2.4",
            minimum_safe_tag: Some("v1.2.0"),
            yanked_tags: &["v1.2.3"],
            expected_state: UpdateCheckState::Yanked,
            expected_json_state: "yanked",
            expected_safety_state: "active_yanked",
            expected_next_command: Some(
                "curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.0/install.sh | sudo sh",
            ),
            expected_replacement_command: Some(
                "curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.0/install.sh | sudo sh",
            ),
        },
        MatrixCase {
            name: "prerelease-active",
            active_tag: "v1.2.5-rc.1",
            latest_tag: "v1.2.4",
            minimum_safe_tag: Some("v1.2.0"),
            yanked_tags: &[],
            expected_state: UpdateCheckState::PrereleaseActive,
            expected_json_state: "prerelease_active",
            expected_safety_state: "safe",
            expected_next_command: None,
            expected_replacement_command: None,
        },
        MatrixCase {
            name: "ineligible-active",
            active_tag: "v1.2.x",
            latest_tag: "v1.2.4",
            minimum_safe_tag: Some("v1.2.0"),
            yanked_tags: &[],
            expected_state: UpdateCheckState::IneligibleActive,
            expected_json_state: "ineligible_active",
            expected_safety_state: "safe",
            expected_next_command: None,
            expected_replacement_command: None,
        },
    ] {
        let output = check_output(
            &active_report(case.active_tag),
            latest(case.latest_tag, case.minimum_safe_tag, case.yanked_tags),
            UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
        );

        assert_eq!(output.state, case.expected_state, "{}", case.name);
        let json: serde_json::Value =
            serde_json::from_str(&crate::json::to_pretty(&output)).expect("update JSON parses");
        assert_eq!(
            json["data"]["state"], case.expected_json_state,
            "{}",
            case.name
        );
        assert_eq!(
            json["data"]["safety_state"], case.expected_safety_state,
            "{}",
            case.name
        );
        let expected_next = case.expected_next_command.unwrap_or("<unavailable>");
        let human = render_human(&output);
        assert!(
            human.contains(&format!("next_command={expected_next}\n")),
            "{}",
            case.name
        );
        let expected_replacement = case.expected_replacement_command.unwrap_or("<unavailable>");
        assert!(
            human.contains(&format!(
                "safety_floor_replacement_command={expected_replacement}\n"
            )),
            "{}",
            case.name
        );
        match case.expected_replacement_command {
            Some(expected) => assert_eq!(
                json["data"]["safety_floor"]["replacement_command"], expected,
                "{}",
                case.name
            ),
            None => assert!(
                json["data"]["safety_floor"]["replacement_command"].is_null(),
                "{}",
                case.name
            ),
        }
    }
}

#[test]
fn stale_safety_fixture_reports_retry_without_repair_command() {
    let metadata = read_freshness_status_artifact_json(&status_artifact_at(
        "v1.2.4",
        Some("v1.2.0"),
        &["v1.1.9"],
        "2026-05-19T11:59:59Z",
    ))
    .expect("stale fixture should parse");
    let output = check_output(
        &active_report("v1.1.9"),
        LatestStatusInput::Available {
            source: "fixture-stale".to_owned(),
            metadata,
            origin: LatestStatusOrigin::CacheFallback,
            offline_reason: Some("remote unavailable".to_owned()),
        },
        UnixSeconds::new(timestamp("2026-05-21T12:00:00Z")),
    );
    let json: serde_json::Value =
        serde_json::from_str(&crate::json::to_pretty(&output)).expect("update JSON parses");

    assert_eq!(json["data"]["state"], "stale_latest_metadata");
    assert_eq!(json["data"]["safety_state"], "stale_metadata");
    let human = render_human(&output);
    assert!(human.contains("next_command=m80 update --check\n"));
    assert!(human.contains("apply_command=<unavailable>\n"));
}

#[test]
fn malformed_safety_fixture_fails_before_json_state() {
    let malformed = status_artifact("v1.2.4", None, &[]).replace(
        r#""minimum_safe_tag":null"#,
        r#""minimum_safe_tag":{"tag":"latest","reason":"bad floor","advisory_url":"https://github.com/moradology/m80/security/advisories/GHSA-test","issue_id":null,"replacement_command":"curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.4/install.sh | sudo sh"}"#,
    );

    let err = parse_latest_status("fixture".to_owned(), malformed)
        .expect_err("malformed safety fixture should fail before update output");

    assert!(err
        .to_string()
        .contains("safety_floor.minimum_safe_tag.tag"));
    assert!(err.to_string().contains("latest"));
}

struct MatrixCase {
    name: &'static str,
    active_tag: &'static str,
    latest_tag: &'static str,
    minimum_safe_tag: Option<&'static str>,
    yanked_tags: &'static [&'static str],
    expected_state: UpdateCheckState,
    expected_json_state: &'static str,
    expected_safety_state: &'static str,
    expected_next_command: Option<&'static str>,
    expected_replacement_command: Option<&'static str>,
}
