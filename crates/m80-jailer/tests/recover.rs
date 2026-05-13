//! Recovery tests using fixture run-dirs — no root or CAP_SYS_ADMIN required.
#![allow(clippy::unwrap_used)]

mod common;

use std::path::Path;

use m80_jailer::{inspect_run_dir, BindMode, Binding, InspectionDecision, Plan};

fn config_with_one_binding(run_dir: &Path) -> m80_jailer::JailerConfig {
    let mut cfg = common::minimal_config(run_dir);
    cfg.bindings = vec![Binding {
        source: "/host/kernel".into(),
        dest: "kernel/vmlinux".into(),
        mode: BindMode::Ro,
    }];
    cfg
}

fn write_state(run_dir: &Path, jailer_pid: Option<u32>, firecracker_pid: Option<u32>) {
    let state = serde_json::json!({
        "jailer_pid": jailer_pid,
        "firecracker_pid": firecracker_pid,
    });
    std::fs::write(
        run_dir.join("jailer-state.json"),
        serde_json::to_vec_pretty(&state).unwrap(),
    )
    .unwrap();
}

fn write_plan(run_dir: &Path) {
    let plan = Plan::compute(&config_with_one_binding(run_dir)).unwrap();
    std::fs::write(
        run_dir.join("jailer-plan.json"),
        serde_json::to_vec_pretty(&plan).unwrap(),
    )
    .unwrap();
}

#[test]
fn no_state_file_returns_no_jail() {
    let dir = tempfile::tempdir().unwrap();
    let decision = inspect_run_dir(dir.path()).unwrap();
    assert!(
        matches!(decision, InspectionDecision::NoJail),
        "got {decision:?}"
    );
}

#[test]
fn plan_without_state_file_returns_orphan_reap_plan() {
    let dir = tempfile::tempdir().unwrap();
    write_plan(dir.path());

    let InspectionDecision::OrphanJail { reap_plan } = inspect_run_dir(dir.path()).unwrap() else {
        panic!("expected OrphanJail");
    };
    let plan = Plan::compute(&config_with_one_binding(dir.path())).unwrap();
    assert_eq!(reap_plan.len(), common::steps(&plan).len());
}

#[test]
fn partial_state_file_with_plan_returns_orphan_reap_plan() {
    let dir = tempfile::tempdir().unwrap();
    write_plan(dir.path());
    std::fs::write(dir.path().join("jailer-state.json"), b"{\"jailer_pid\":").unwrap();

    let InspectionDecision::OrphanJail { reap_plan } = inspect_run_dir(dir.path()).unwrap() else {
        panic!("expected OrphanJail");
    };
    let plan = Plan::compute(&config_with_one_binding(dir.path())).unwrap();
    assert_eq!(reap_plan.len(), common::steps(&plan).len());
}

#[test]
fn partial_state_file_without_plan_returns_no_jail() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("jailer-state.json"), b"{\"jailer_pid\":").unwrap();

    let decision = inspect_run_dir(dir.path()).unwrap();

    assert!(
        matches!(decision, InspectionDecision::NoJail),
        "got {decision:?}"
    );
}

#[test]
fn stale_state_with_nonexistent_pids_returns_orphan() {
    let dir = tempfile::tempdir().unwrap();
    write_state(dir.path(), Some(u32::MAX), Some(u32::MAX - 1));
    write_plan(dir.path());

    let decision = inspect_run_dir(dir.path()).unwrap();
    assert!(
        matches!(decision, InspectionDecision::OrphanJail { .. }),
        "got {decision:?}"
    );
}

#[test]
fn orphan_reap_plan_matches_plan_len() {
    let dir = tempfile::tempdir().unwrap();
    write_state(dir.path(), Some(u32::MAX), Some(u32::MAX - 1));
    write_plan(dir.path());

    let InspectionDecision::OrphanJail { reap_plan } = inspect_run_dir(dir.path()).unwrap() else {
        panic!("expected OrphanJail");
    };
    let plan = Plan::compute(&config_with_one_binding(dir.path())).unwrap();
    assert_eq!(reap_plan.len(), common::steps(&plan).len());
}

#[test]
fn live_state_with_own_pid_returns_live_jail() {
    let dir = tempfile::tempdir().unwrap();
    let self_pid = std::process::id();
    write_state(dir.path(), Some(self_pid), Some(self_pid));

    let decision = inspect_run_dir(dir.path()).unwrap();
    assert!(
        matches!(
            decision,
            InspectionDecision::LiveJail { jailer_pid, firecracker_pid }
                if jailer_pid == self_pid && firecracker_pid == self_pid
        ),
        "got {decision:?}"
    );
}

#[test]
fn new_pid_ns_state_with_no_live_jailer_returns_live_jail() {
    let dir = tempfile::tempdir().unwrap();
    let self_pid = std::process::id();
    write_state(dir.path(), Some(0), Some(self_pid));

    let decision = inspect_run_dir(dir.path()).unwrap();
    assert!(
        matches!(
            decision,
            InspectionDecision::LiveJail { jailer_pid, firecracker_pid }
                if jailer_pid == 0 && firecracker_pid == self_pid
        ),
        "got {decision:?}"
    );
}

#[test]
fn mixed_live_and_dead_pid_returns_orphan() {
    let dir = tempfile::tempdir().unwrap();
    write_state(dir.path(), Some(std::process::id()), Some(u32::MAX));
    write_plan(dir.path());

    let decision = inspect_run_dir(dir.path()).unwrap();
    assert!(
        matches!(decision, InspectionDecision::OrphanJail { .. }),
        "got {decision:?}"
    );
}

#[test]
fn orphan_without_plan_file_has_empty_reap_plan() {
    let dir = tempfile::tempdir().unwrap();
    write_state(dir.path(), Some(u32::MAX), Some(u32::MAX - 1));

    let InspectionDecision::OrphanJail { reap_plan } = inspect_run_dir(dir.path()).unwrap() else {
        panic!("expected OrphanJail");
    };
    assert!(reap_plan.is_empty(), "got {reap_plan:?}");
}
