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
        stdout.contains("Install release artifacts")
            && stdout.contains("--artifact-url")
            && stdout.contains("--no-run"),
        "quickstart help should expose the release artifact flow, got: {stdout}"
    );
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
fn help_cleanup() {
    m80().args(["cleanup", "--help"]).assert().success();
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
        stdout.contains("--foreground") && stdout.contains("--size"),
        "warm enable help should expose foreground owner controls, got: {stdout}"
    );
}

#[test]
fn help_version() {
    m80().args(["version", "--help"]).assert().success();
}
