use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::super::latest_status::latest_status_input;
use super::http_fixture::HttpFixture;
use super::*;
use crate::args::UpdateArgs;

#[test]
fn freshness_fixture_harness_serves_every_remote_status_shape() {
    let fixture = FreshnessFixture::new();

    fixture.add_status(
        "current-stable-v1.2.3",
        status_artifact_at("v1.2.3", None, &[], "2026-05-21T12:00:00Z"),
    );
    fixture.add_status(
        "outdated-stable-v1.2.3-to-v1.2.4-with-floor",
        status_artifact_at("v1.2.4", Some("v1.2.0"), &[], "2026-05-21T12:00:00Z"),
    );
    fixture.add_status(
        "stale-status-v1.2.4",
        status_artifact_at("v1.2.4", None, &[], "2026-05-19T11:59:59Z"),
    );
    fixture.add_malformed(
        "malformed-missing-resolved-tag",
        status_artifact_with_resolved_tag(None, None, &[]),
    );
    fixture.add_unavailable("offline-status-v1.2.4");
    fixture.assert_public_curl_blocked();

    let current = check_output(
        &active_report("v1.2.3"),
        fixture.latest("current-stable-v1.2.3"),
        UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
    );
    assert_eq!(current.state, UpdateCheckState::Current);
    assert_eq!(current.latest_status_origin, LatestStatusOrigin::Remote);
    assert_eq!(
        current.latest_status_source,
        fixture.url("current-stable-v1.2.3")
    );
    assert_eq!(
        current.next_command.as_deref(),
        Some("m80 run -- echo hello")
    );

    let outdated = check_output(
        &active_report("v1.2.3"),
        fixture.latest("outdated-stable-v1.2.3-to-v1.2.4-with-floor"),
        UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
    );
    assert_eq!(outdated.state, UpdateCheckState::Outdated);
    assert_eq!(outdated.safety_state, SafetyFloorStatus::Safe);
    assert_eq!(
        outdated.next_command.as_deref(),
        Some(
            "curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.4/install.sh | sudo sh"
        )
    );

    let stale = check_output(
        &active_report("v1.2.3"),
        fixture.latest("stale-status-v1.2.4"),
        UnixSeconds::new(timestamp("2026-05-21T12:00:00Z")),
    );
    assert_eq!(stale.state, UpdateCheckState::StaleLatestMetadata);
    assert_eq!(
        stale.latest_status_cache_state,
        LatestStatusCacheState::NotUsed
    );
    assert_eq!(stale.retry_command.as_deref(), Some("m80 update --check"));

    let malformed = latest_status_input(&fixture.args("malformed-missing-resolved-tag"))
        .expect_err("malformed status fixture should fail closed");
    assert!(malformed.to_string().contains("missing resolved_tag"));

    let offline = latest_status_input(&fixture.args("offline-status-v1.2.4"))
        .expect("offline fixture should return unknown_offline input");
    let offline = check_output(
        &active_report("v1.2.3"),
        offline,
        UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
    );
    assert_eq!(offline.state, UpdateCheckState::UnknownOffline);
    assert_eq!(
        offline.latest_status_origin,
        LatestStatusOrigin::Unavailable
    );
    assert!(offline
        .latest_status_error
        .as_deref()
        .unwrap()
        .contains("latest status fetch failed"));

    fixture.assert_only_fixture_requests();
}

#[test]
fn freshness_fixture_harness_covers_active_install_classifiers() {
    let fixture = FreshnessFixture::new();
    fixture.add_status(
        "classifier-latest-v1.2.4",
        status_artifact_at("v1.2.4", None, &[], "2026-05-21T12:00:00Z"),
    );

    for case in [
        ClassifierCase {
            name: "prerelease-active-v1.2.5-rc.1",
            report: active_report("v1.2.5-rc.1"),
            expected_kind: ActiveInstallKind::Prerelease,
            expected_state: UpdateCheckState::PrereleaseActive,
        },
        ClassifierCase {
            name: "ineligible-active-v1.2.x",
            report: active_report("v1.2.x"),
            expected_kind: ActiveInstallKind::Ineligible,
            expected_state: UpdateCheckState::IneligibleActive,
        },
        ClassifierCase {
            name: "local-dev-active",
            report: local_dev_report(),
            expected_kind: ActiveInstallKind::LocalDev,
            expected_state: UpdateCheckState::LocalDevInstall,
        },
        ClassifierCase {
            name: "stale-active-metadata-v1.2.3",
            report: stale_active_report("v1.2.3"),
            expected_kind: ActiveInstallKind::StaleActiveMetadata,
            expected_state: UpdateCheckState::InstallUnhealthy,
        },
    ] {
        let output = check_output(
            &case.report,
            fixture.latest("classifier-latest-v1.2.4"),
            UnixSeconds::new(timestamp("2026-05-21T12:30:00Z")),
        );

        assert_eq!(output.active_kind, case.expected_kind, "{}", case.name);
        assert_eq!(output.state, case.expected_state, "{}", case.name);
        assert_eq!(output.freshness_state, case.expected_state, "{}", case.name);
    }

    fixture.assert_only_fixture_requests();
}

struct ClassifierCase {
    name: &'static str,
    report: InstallStateReport,
    expected_kind: ActiveInstallKind,
    expected_state: UpdateCheckState,
}

struct FreshnessFixture {
    server: HttpFixture,
    _curl_guard: LoopbackCurlGuard,
}

impl FreshnessFixture {
    fn new() -> Self {
        Self {
            server: HttpFixture::new(),
            _curl_guard: LoopbackCurlGuard::new(),
        }
    }

    fn add_status(&self, scenario: &str, body: String) {
        self.server.add_ok(
            &Self::path(scenario),
            self.with_asset_traps(body).into_bytes(),
        );
    }

    fn add_malformed(&self, scenario: &str, body: String) {
        self.server.add_ok(&Self::path(scenario), body.into_bytes());
    }

    fn add_unavailable(&self, scenario: &str) {
        self.server.add_status(
            &Self::path(scenario),
            503,
            format!("fixture scenario unavailable: {scenario}\n").into_bytes(),
        );
    }

    fn latest(&self, scenario: &str) -> LatestStatusInput {
        latest_status_input(&self.args(scenario)).expect("fixture latest status should parse")
    }

    fn args(&self, scenario: &str) -> UpdateArgs {
        UpdateArgs {
            check: true,
            install_root: PathBuf::from("/tmp/m80-install"),
            profile: None,
            latest_status: None,
            latest_status_url: Some(self.url(scenario)),
            config_path: None,
            profile_dir: None,
        }
    }

    fn url(&self, scenario: &str) -> String {
        let url = self.server.url(&Self::path(scenario));
        assert!(
            url.starts_with("http://127.0.0.1:"),
            "freshness fixture must only hand out loopback URLs: {url}"
        );
        url
    }

    fn assert_only_fixture_requests(&self) {
        let requests = self.server.requests();
        assert!(!requests.is_empty(), "fixture should have served requests");
        for request in requests {
            assert!(
                request.starts_with("/freshness/"),
                "unexpected non-fixture request path: {request}"
            );
            assert!(
                !request.contains("github.com") && !request.contains("releases/download"),
                "fixture leaked a public release URL into the HTTP request path: {request}"
            );
            assert!(
                !request.starts_with("/public-asset-trap/"),
                "update --check fetched a status-advertised asset instead of only reading metadata: {request}"
            );
        }
    }

    fn with_asset_traps(&self, body: String) -> String {
        let mut value: serde_json::Value =
            serde_json::from_str(&body).expect("status fixture JSON should parse before traps");
        if let Some(rows) = value
            .get_mut("checked_urls")
            .and_then(|rows| rows.as_array_mut())
        {
            for (index, row) in rows.iter_mut().enumerate() {
                row["url"] =
                    serde_json::Value::String(self.trap_url(&format!("checked-url-{index}")));
            }
        }
        if let Some(rows) = value
            .get_mut("public_assets")
            .and_then(|rows| rows.as_array_mut())
        {
            for (index, row) in rows.iter_mut().enumerate() {
                row["url"] =
                    serde_json::Value::String(self.trap_url(&format!("public-asset-{index}")));
            }
        }
        serde_json::to_string(&value).expect("status fixture JSON should render after traps")
    }

    fn trap_url(&self, name: &str) -> String {
        let url = self.server.url(&format!("/public-asset-trap/{name}"));
        assert!(
            url.starts_with("http://127.0.0.1:"),
            "freshness asset trap must only hand out loopback URLs: {url}"
        );
        url
    }

    fn assert_public_curl_blocked(&self) {
        let output = Command::new("curl")
            .arg("-fsSL")
            .arg("https://github.com/moradology/m80/releases/latest/download/install.sh")
            .output()
            .expect("run loopback-only curl wrapper");
        assert_eq!(output.status.code(), Some(97));
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("blocked non-loopback curl URL"),
            "unexpected fake curl stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn path(scenario: &str) -> String {
        assert!(
            scenario
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_')),
            "fixture scenario names must stay URL-path safe: {scenario}"
        );
        format!("/freshness/{scenario}.json")
    }
}

fn local_dev_report() -> InstallStateReport {
    let mut report = active_report("v1.2.3");
    report.state = InstallStateKind::LocalDevTree;
    report.active_pointer.release_tag = None;
    report.metadata = None;
    report
}

fn stale_active_report(tag: &str) -> InstallStateReport {
    let mut report = active_report(tag);
    report.state = InstallStateKind::StaleInstallMetadata;
    report
}

struct LoopbackCurlGuard {
    _temp: tempfile::TempDir,
    _env_restore: m80_test_helpers::env::EnvRestore,
    _env_lock: std::sync::MutexGuard<'static, ()>,
}

impl LoopbackCurlGuard {
    fn new() -> Self {
        let env_lock = m80_test_helpers::env::env_lock().lock().unwrap();
        let env_restore = m80_test_helpers::env::EnvRestore::capture(&["PATH", "M80_REAL_CURL"]);
        let real_curl = real_curl_path();
        let temp = tempfile::tempdir().expect("create fake curl tempdir");
        write_loopback_curl(temp.path(), &real_curl);
        let old_path = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![temp.path().to_path_buf()];
        paths.extend(std::env::split_paths(&old_path));
        let new_path = std::env::join_paths(paths).expect("join fake curl PATH");
        std::env::set_var("M80_REAL_CURL", real_curl);
        std::env::set_var("PATH", new_path);
        Self {
            _temp: temp,
            _env_restore: env_restore,
            _env_lock: env_lock,
        }
    }
}

fn real_curl_path() -> PathBuf {
    let output = Command::new("which")
        .arg("curl")
        .output()
        .expect("locate real curl");
    assert!(output.status.success(), "which curl failed");
    PathBuf::from(String::from_utf8(output.stdout).unwrap().trim())
}

fn write_loopback_curl(dir: &Path, real_curl: &Path) {
    let path = dir.join("curl");
    fs::write(
        &path,
        format!(
            r#"#!/usr/bin/env sh
set -eu
url=''
for arg in "$@"; do
  case "$arg" in
    http://*|https://*) url="$arg" ;;
  esac
done
case "$url" in
  http://127.0.0.1:*|http://localhost:*|https://127.0.0.1:*|https://localhost:*) exec '{}' "$@" ;;
  *) printf 'blocked non-loopback curl URL: %s\n' "$url" >&2; exit 97 ;;
esac
"#,
            real_curl.display()
        ),
    )
    .expect("write fake curl");
    let mut perms = fs::metadata(&path)
        .expect("fake curl metadata")
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).expect("make fake curl executable");
}
