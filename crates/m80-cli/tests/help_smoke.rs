//! Smoke tests: `--help` for every subcommand renders without error.
//!
//! Uses `assert_cmd` to run the actual binary.

use assert_cmd::Command;

fn m80() -> Command {
    Command::cargo_bin("m80").unwrap()
}

#[test]
fn help_toplevel() {
    m80().arg("--help").assert().success();
}

#[test]
fn help_preflight() {
    m80().args(["preflight", "--help"]).assert().success();
}

#[test]
fn help_launch() {
    m80().args(["launch", "--help"]).assert().success();
}

#[test]
fn help_exec() {
    m80().args(["exec", "--help"]).assert().success();
}

#[test]
fn help_stop() {
    m80().args(["stop", "--help"]).assert().success();
}

#[test]
fn help_inspect() {
    m80().args(["inspect", "--help"]).assert().success();
}

#[test]
fn help_list() {
    m80().args(["list", "--help"]).assert().success();
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
fn help_version() {
    m80().args(["version", "--help"]).assert().success();
}
