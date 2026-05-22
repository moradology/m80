use std::fs;

use super::super::latest_status::{
    latest_status_from_cache_after_fetch_failure, latest_status_input,
};
use super::http_fixture::HttpFixture;
use super::*;
use crate::args::UpdateArgs;

#[test]
fn update_check_uses_fresh_cache_when_remote_latest_is_unavailable() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("latest-status.json");
    fs::write(
        &cache,
        status_artifact_at("v1.2.3", None, &[], "2026-05-21T12:00:00Z"),
    )
    .unwrap();

    let latest = latest_status_from_cache_after_fetch_failure(
        &cache,
        "https://example.invalid/status.json".to_owned(),
        "network unavailable".to_owned(),
    )
    .expect("cache fallback should parse");
    let output = check_output(
        &active_report("v1.2.3"),
        latest,
        UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
    );

    assert_eq!(output.state, UpdateCheckState::Current);
    assert_eq!(
        output.latest_status_origin,
        LatestStatusOrigin::CacheFallback
    );
    assert_eq!(
        output.latest_status_cache_state,
        LatestStatusCacheState::Fresh
    );
    assert_eq!(
        output.latest_status_fetched_at,
        Some(timestamp("2026-05-21T12:00:00Z"))
    );
    assert_eq!(output.latest_status_max_age_seconds, 172_800);
    assert_eq!(output.latest_status_error, None);
    assert!(output
        .latest_status_offline_reason
        .as_deref()
        .unwrap()
        .contains("network unavailable"));
    assert_eq!(
        output.next_command.as_deref(),
        Some("m80 run -- echo hello")
    );
}

#[test]
fn update_check_reports_stale_cache_when_remote_latest_is_unavailable() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("latest-status.json");
    fs::write(
        &cache,
        status_artifact_at("v1.2.3", None, &[], "2026-05-19T11:59:59Z"),
    )
    .unwrap();

    let latest = latest_status_from_cache_after_fetch_failure(
        &cache,
        "https://example.invalid/status.json".to_owned(),
        "network unavailable".to_owned(),
    )
    .expect("stale cache fallback should parse");
    let output = check_output(
        &active_report("v1.2.3"),
        latest,
        UnixSeconds::new(timestamp("2026-05-21T12:00:00Z")),
    );

    assert_eq!(output.state, UpdateCheckState::StaleLatestMetadata);
    assert_eq!(
        output.latest_status_origin,
        LatestStatusOrigin::CacheFallback
    );
    assert_eq!(
        output.latest_status_cache_state,
        LatestStatusCacheState::Stale
    );
    assert_eq!(output.retry_command.as_deref(), Some("m80 update --check"));
    assert_eq!(output.next_command, output.retry_command);
    let human = render_human(&output);
    assert!(human.contains("latest_status_cache_state=stale\n"));
    assert!(human.contains("retry_command=m80 update --check\n"));
    assert!(
        human.contains("message=latest status is stale; rerun freshness before applying updates\n")
    );
}

#[test]
fn update_check_reports_missing_cache_when_remote_latest_is_unavailable() {
    let cache = tempfile::tempdir()
        .unwrap()
        .path()
        .join("missing-status.json");

    let latest = latest_status_from_cache_after_fetch_failure(
        &cache,
        "https://example.invalid/status.json".to_owned(),
        "network unavailable".to_owned(),
    )
    .expect("missing cache should become offline status");
    let output = check_output(
        &active_report("v1.2.3"),
        latest,
        UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
    );

    assert_eq!(output.state, UpdateCheckState::UnknownOffline);
    assert_eq!(output.latest_status_origin, LatestStatusOrigin::Unavailable);
    assert!(
        output.latest_status_source.starts_with("file:"),
        "{:?}",
        output.latest_status_source
    );
    assert_eq!(
        output.latest_status_cache_state,
        LatestStatusCacheState::Missing
    );
    assert_eq!(output.latest_status_fetched_at, None);
    assert!(output
        .latest_status_offline_reason
        .as_deref()
        .unwrap()
        .contains("cached latest status unavailable"));
}

#[test]
fn update_check_reports_malformed_cache_when_remote_latest_is_unavailable() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("latest-status.json");
    fs::write(&cache, "{}").unwrap();

    let latest = latest_status_from_cache_after_fetch_failure(
        &cache,
        "https://example.invalid/status.json".to_owned(),
        "network unavailable".to_owned(),
    )
    .expect("malformed cache should become offline status");
    let output = check_output(
        &active_report("v1.2.3"),
        latest,
        UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
    );

    assert_eq!(output.state, UpdateCheckState::UnknownOffline);
    assert_eq!(output.latest_status_origin, LatestStatusOrigin::Unavailable);
    assert!(
        output.latest_status_source.starts_with("file:"),
        "{:?}",
        output.latest_status_source
    );
    assert_eq!(
        output.latest_status_cache_state,
        LatestStatusCacheState::Malformed
    );
    assert!(output
        .latest_status_offline_reason
        .as_deref()
        .unwrap()
        .contains("cached latest status malformed"));
}

#[test]
fn latest_status_input_reports_missing_fallback_cache_after_remote_failure() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("missing-status.json");
    let server = HttpFixture::new();
    let url = server.url("/latest-status.json");

    let latest = latest_status_input(&update_args(cache.clone(), url.clone()))
        .expect("missing cache should become offline status");

    match latest {
        LatestStatusInput::UnknownOffline {
            source,
            detail,
            origin,
            cache_state,
        } => {
            assert_eq!(source, format!("file:{}", cache.display()));
            assert_eq!(origin, LatestStatusOrigin::Unavailable);
            assert_eq!(cache_state, LatestStatusCacheState::Missing);
            assert!(detail.contains(&url), "{detail}");
            assert!(
                detail.contains(&format!("file:{}", cache.display())),
                "{detail}"
            );
            assert!(
                detail.contains("cached latest status unavailable"),
                "{detail}"
            );
        }
        other => panic!("unexpected latest status input: {other:?}"),
    }
}

#[test]
fn latest_status_input_reports_malformed_fallback_cache_after_remote_failure() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("latest-status.json");
    fs::write(&cache, "{}").unwrap();
    let server = HttpFixture::new();
    let url = server.url("/latest-status.json");

    let latest = latest_status_input(&update_args(cache.clone(), url.clone()))
        .expect("malformed cache should become offline status");

    match latest {
        LatestStatusInput::UnknownOffline {
            source,
            detail,
            origin,
            cache_state,
        } => {
            assert_eq!(source, format!("file:{}", cache.display()));
            assert_eq!(origin, LatestStatusOrigin::Unavailable);
            assert_eq!(cache_state, LatestStatusCacheState::Malformed);
            assert!(detail.contains(&url), "{detail}");
            assert!(
                detail.contains(&format!("file:{}", cache.display())),
                "{detail}"
            );
            assert!(
                detail.contains("cached latest status malformed"),
                "{detail}"
            );
        }
        other => panic!("unexpected latest status input: {other:?}"),
    }
}

fn update_args(latest_status: std::path::PathBuf, latest_status_url: String) -> UpdateArgs {
    UpdateArgs {
        check: true,
        install_root: std::path::PathBuf::from("/tmp/m80-install"),
        profile: None,
        latest_status: Some(latest_status),
        latest_status_url: Some(latest_status_url),
        config_path: None,
        profile_dir: None,
    }
}
