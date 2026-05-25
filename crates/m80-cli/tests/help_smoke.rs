//! Smoke tests: `--help` for every subcommand renders without error.
//!
//! Uses `assert_cmd` to run the actual binary.

mod common;

use common::m80;

#[test]
fn help_toplevel() {
    let output = m80().arg("--help").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("Run a process with a constrained view of the host"),
        "top-level help should lead with the constrained-process model, got: {stdout}"
    );
}

#[test]
fn help_run() {
    let output = m80().args(["run", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("Run one process inside an m80 sandbox")
            && stdout.contains("Runtime image/profile name"),
        "run help should lead with the process wrapper and profile model, got: {stdout}"
    );
}

#[test]
fn help_preflight() {
    m80().args(["preflight", "--help"]).assert().success();
}

#[test]
fn help_quickstart() {
    let output = m80().args(["quickstart", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("Install an explicit artifact tarball for operator/test overrides")
            && stdout.contains(latest_install_command())
            && stdout.contains(pinned_install_command())
            && stdout.contains("matching this m80 binary")
            && stdout.contains("not the normal first-run path")
            && stdout.contains("legacy-quickstart-hard-cutover.md")
            && stdout.contains("--artifact-url")
            && stdout.contains("--no-run"),
        "quickstart help should expose the override-only artifact flow, got: {stdout}"
    );
    assert_public_help_order("quickstart help", &stdout, "--artifact-url");
    assert_no_stale_public_quickstart_urls("quickstart help", &stdout);
}

#[test]
fn help_install() {
    let output = m80().args(["install", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("Install or plan a release bundle")
            && stdout.contains(latest_install_command())
            && stdout.contains(pinned_install_command())
            && stdout.contains("--release-tag")
            && stdout.contains("--bundle-url")
            && stdout.contains("explicit operator/test override")
            && stdout.contains("verify bundle compatibility")
            && stdout.contains("--install-root")
            && stdout.contains("--dry-run")
            && !stdout.contains("--bootstrap-tag"),
        "install help should expose user-facing installer inputs only, got: {stdout}"
    );
    assert_public_help_order("install help", &stdout, "--bundle-url");
    assert_no_stale_public_quickstart_urls("install help", &stdout);
}

#[test]
fn help_inspect() {
    m80().args(["inspect", "--help"]).assert().success();
}

#[test]
fn help_logs() {
    let output = m80().args(["logs", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("out-of-band VM diagnostics") && stdout.contains("--request-id"),
        "logs help should frame persisted diagnostics, got: {stdout}"
    );
}

#[test]
fn help_list() {
    m80().args(["list", "--help"]).assert().success();
}

#[test]
fn help_env() {
    let output = m80().args(["env", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("host capabilities") && stdout.contains("runtime paths"),
        "env help should expose the diagnostic dump scope, got: {stdout}"
    );
}

#[test]
fn help_bug_report() {
    let output = m80().args(["bug-report", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("redacted support bundle")
            && stdout.contains("--vm-id")
            && stdout.contains("--log-tail-lines"),
        "bug-report help should expose the single support bundle path, got: {stdout}"
    );
}

#[test]
fn help_cleanup() {
    m80().args(["cleanup", "--help"]).assert().success();
}

#[test]
fn help_net() {
    let output = m80().args(["net", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("Inspect or clean m80-owned host network residue"),
        "net help should frame network residue cleanup, got: {stdout}"
    );
}

#[test]
fn help_net_cleanup() {
    let output = m80().args(["net", "cleanup", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("m80-tagged iptables rules")
            && stdout.contains("tfc* TAPs")
            && stdout.contains("--dry-run"),
        "net cleanup help should expose tag-scan cleanup and dry-run, got: {stdout}"
    );
}

#[test]
fn help_install_cleanup() {
    m80().args(["install-cleanup", "--help"]).assert().success();
}

#[test]
fn help_config() {
    m80().args(["config", "--help"]).assert().success();
}

#[test]
fn help_config_show() {
    m80().args(["config", "show", "--help"]).assert().success();
}

#[test]
fn help_update() {
    let output = m80().args(["update", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("Check or update the installed release")
            && stdout.contains("--check")
            && stdout.contains("--install-root")
            && stdout.contains("--latest-status")
            && stdout.contains("--latest-status-url"),
        "update help should expose the read-only check surface, got: {stdout}"
    );
}

#[test]
fn help_warm() {
    let output = m80().args(["warm", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("Explicit warm-sandbox owner control"),
        "warm help should frame warm as explicit owner control, got: {stdout}"
    );
}

#[test]
fn help_warm_enable() {
    let output = m80().args(["warm", "enable", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("--size"),
        "warm enable help should expose --size, got: {stdout}"
    );
}

#[test]
fn help_image() {
    let output = m80().args(["image", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("Manage content-addressed image-store artifacts"),
        "image help should frame the image-store surface, got: {stdout}"
    );
}

#[test]
fn help_image_actions() {
    for action in ["build", "gc", "list", "show", "rm", "verify"] {
        m80().args(["image", action, "--help"]).assert().success();
    }
}

#[test]
fn help_template() {
    let output = m80().args(["template", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("Manage snapshot-template store artifacts"),
        "template help should frame the snapshot-template surface, got: {stdout}"
    );
}

#[test]
fn help_template_actions() {
    for action in ["build", "list", "show", "prune", "rm"] {
        m80()
            .args(["template", action, "--help"])
            .assert()
            .success();
    }
}

#[test]
fn help_version() {
    m80().args(["version", "--help"]).assert().success();
}

fn assert_no_stale_public_quickstart_urls(surface: &str, text: &str) {
    assert!(
        !text.contains("raw.githubusercontent.com") && !text.contains("/main/install.sh"),
        "{surface} must not point users at mutable raw main installers, got: {text}"
    );
    assert!(
        !text
            .split_ascii_whitespace()
            .any(|token| token.contains("/releases/latest/download/")
                && !token.contains("/releases/latest/download/install.sh")),
        "{surface} must not point users at artifact-only latest release assets, got: {text}"
    );
}

fn assert_public_help_order(surface: &str, text: &str, override_marker: &str) {
    let latest = text
        .find(latest_install_command())
        .unwrap_or_else(|| panic!("{surface} missing latest install command: {text}"));
    let pinned = text
        .find(pinned_install_command())
        .unwrap_or_else(|| panic!("{surface} missing pinned install command: {text}"));
    let override_pos = text
        .find(override_marker)
        .unwrap_or_else(|| panic!("{surface} missing override marker {override_marker}: {text}"));
    assert!(
        latest < pinned && pinned < override_pos,
        "{surface} must order common latest, pinned, then explicit override, got: {text}"
    );
}

fn latest_install_command() -> &'static str {
    "curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh"
}

fn pinned_install_command() -> &'static str {
    "curl -fsSL https://github.com/moradology/m80/releases/download/<version>/install.sh | sudo sh"
}
