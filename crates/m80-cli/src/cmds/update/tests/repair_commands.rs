use std::{fs, path::PathBuf};

use super::super::{
    check_output, pinned_install_command, render_human, LatestStatusCacheState, LatestStatusInput,
    LatestStatusOrigin, UpdateCheckState,
};
use super::*;
use crate::release_freshness::read_freshness_status_artifact_json;

#[test]
fn unsafe_active_release_prints_one_pinned_repair_command() {
    let output = check_output(
        &active_report("v1.1.9"),
        latest("v1.2.4", Some("v1.2.0"), &[]),
        UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
    );
    let expected = pinned_install_command("v1.2.0");

    assert_eq!(output.state, UpdateCheckState::Unsafe);
    assert_eq!(output.apply_command.as_deref(), Some(expected.as_str()));
    assert_eq!(output.reinstall_command, None);
    assert_eq!(output.retry_command, None);
    assert_eq!(output.next_command.as_deref(), Some(expected.as_str()));

    let human = render_human(&output);
    assert!(human.contains("safety_state=active_below_minimum\n"));
    assert!(human.contains("safety_floor_status=active_below_minimum\n"));
    assert!(human.contains("safety_floor_policy_tag=v1.2.0\n"));
    assert!(human.contains("safety_floor_reason=security floor\n"));
    assert!(human.contains(&format!("safety_floor_replacement_command={expected}\n")));
    assert!(human.contains(&format!("apply_command={expected}\n")));
    assert!(human.contains(&format!("next_command={expected}\n")));
}

#[test]
fn unstable_or_ineligible_active_tags_do_not_emit_stable_update_commands() {
    for (tag, state) in [
        ("v1.2.3-rc.1", UpdateCheckState::PrereleaseActive),
        ("v1.2.3+build.1", UpdateCheckState::IneligibleActive),
        ("v1.2.x;sudo", UpdateCheckState::IneligibleActive),
    ] {
        let output = check_output(
            &active_report(tag),
            latest("v1.2.4", Some("v1.2.0"), &[]),
            UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
        );

        assert_eq!(output.state, state, "active tag {tag}");
        assert_eq!(output.apply_command, None, "active tag {tag}");
        assert_eq!(output.reinstall_command, None, "active tag {tag}");
        assert_eq!(output.retry_command, None, "active tag {tag}");
        assert_eq!(output.next_command, None, "active tag {tag}");
        assert!(
            !render_human(&output).contains("install.sh | sudo sh"),
            "active tag {tag}"
        );
    }
}

#[test]
fn local_dev_and_unknown_offline_never_emit_stable_apply_commands() {
    let mut local_dev = active_report("v1.2.3");
    local_dev.state = InstallStateKind::LocalDevTree;
    local_dev.active_pointer.release_tag = None;
    local_dev.metadata = None;
    let local_output = check_output(
        &local_dev,
        latest("v1.2.4", Some("v1.2.0"), &[]),
        UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
    );

    assert_eq!(local_output.state, UpdateCheckState::LocalDevInstall);
    assert_eq!(local_output.apply_command, None);
    assert_eq!(local_output.reinstall_command, None);
    assert_eq!(local_output.retry_command, None);
    assert_eq!(local_output.next_command, None);

    let offline_output = check_output(
        &active_report("v1.2.3"),
        LatestStatusInput::UnknownOffline {
            source: "https://example.invalid/status.json".to_owned(),
            detail: "network unavailable".to_owned(),
            origin: LatestStatusOrigin::Unavailable,
            cache_state: LatestStatusCacheState::NotConfigured,
        },
        UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
    );

    assert_eq!(offline_output.state, UpdateCheckState::UnknownOffline);
    assert_eq!(offline_output.apply_command, None);
    assert_eq!(offline_output.reinstall_command, None);
    assert_eq!(
        offline_output.retry_command.as_deref(),
        Some("m80 update --check")
    );
    assert_eq!(offline_output.next_command, offline_output.retry_command);
    assert!(!render_human(&offline_output).contains("install.sh | sudo sh"));
}

#[test]
fn latest_status_rejects_tags_that_would_need_shell_quoting_before_command_rendering() {
    for tag in ["v1.2.4-rc.1", "v1.2.4+build.1", "v1.2.x;sudo"] {
        let err = read_freshness_status_artifact_json(&status_artifact(tag, None, &[]))
            .expect_err("non-stable latest tag should fail before command rendering");

        assert!(
            err.to_string()
                .contains("freshness status resolved_tag is not stable"),
            "tag {tag} returned {err}"
        );
    }
}

#[test]
fn public_docs_show_the_renderer_command_text() {
    let command = pinned_install_command("v1.2.4");

    for path in [
        "crates/m80-cli/README.md",
        "docs/runbook/release.md",
        "docs/behaviors/release/update-check.md",
    ] {
        let text = fs::read_to_string(repo_root().join(path)).expect("read repo doc");
        assert!(
            text.contains(&format!("next_command={command}"))
                || text.contains(&format!("apply_command={command}")),
            "{path} missing renderer command {command:?}"
        );
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("m80-cli crate should be under crates/")
        .to_path_buf()
}
