//! Recovery tests using fixture run-dirs — no root or CAP_SYS_ADMIN required.

use m80_jailer::{BindMode, Binding, JailerConfig, Plan, RecoveryDecision, recover_from_run_dir};
use std::path::PathBuf;

fn sample_config(run_dir: &std::path::Path) -> JailerConfig {
    JailerConfig {
        jailer_bin: PathBuf::from("/usr/bin/jailer"),
        firecracker_bin: PathBuf::from("/usr/bin/firecracker"),
        run_dir: run_dir.to_path_buf(),
        uid: 3000,
        gid: 3000,
        bindings: vec![Binding {
            source: PathBuf::from("/host/kernel"),
            dest: PathBuf::from("kernel/vmlinux"),
            mode: BindMode::Ro,
        }],
        sockets: Vec::new(),
    }
}

fn write_state(run_dir: &std::path::Path, jailer_pid: Option<u32>, firecracker_pid: Option<u32>) {
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

fn write_plan(run_dir: &std::path::Path) {
    let cfg = sample_config(run_dir);
    let plan = Plan::compute(&cfg).unwrap();
    std::fs::write(
        run_dir.join("jailer-plan.json"),
        serde_json::to_vec_pretty(&plan).unwrap(),
    )
    .unwrap();
}

#[test]
fn no_state_file_returns_no_jail() {
    let dir = tempfile::tempdir().unwrap();
    let decision = recover_from_run_dir(dir.path()).unwrap();
    assert!(
        matches!(decision, RecoveryDecision::NoJail),
        "expected NoJail when no state file present, got {decision:?}"
    );
}

#[test]
fn stale_state_with_nonexistent_pids_returns_orphan() {
    let dir = tempfile::tempdir().unwrap();
    // PID u32::MAX is extremely unlikely to be a live process.
    write_state(dir.path(), Some(u32::MAX), Some(u32::MAX - 1));
    write_plan(dir.path());

    let decision = recover_from_run_dir(dir.path()).unwrap();
    assert!(
        matches!(decision, RecoveryDecision::OrphanJail { .. }),
        "expected OrphanJail for nonexistent pids, got {decision:?}"
    );
}

#[test]
fn orphan_reap_steps_are_plan_steps_reversed() {
    let dir = tempfile::tempdir().unwrap();
    write_state(dir.path(), Some(u32::MAX), Some(u32::MAX - 1));
    write_plan(dir.path());

    let decision = recover_from_run_dir(dir.path()).unwrap();
    if let RecoveryDecision::OrphanJail { reap_steps } = decision {
        // Load the forward plan to compare.
        let cfg = sample_config(dir.path());
        let plan = Plan::compute(&cfg).unwrap();
        let forward_json: Vec<String> = plan
            .steps
            .iter()
            .map(|s| serde_json::to_string(s).unwrap())
            .collect();
        let reap_json: Vec<String> = reap_steps
            .iter()
            .map(|s| serde_json::to_string(s).unwrap())
            .collect();
        let reversed_forward: Vec<&String> = forward_json.iter().rev().collect();
        let reap_refs: Vec<&String> = reap_json.iter().collect();
        assert_eq!(
            reversed_forward, reap_refs,
            "reap_steps must be plan.steps in reverse"
        );
    } else {
        panic!("expected OrphanJail, got {decision:?}");
    }
}

#[test]
fn live_state_with_own_pid_returns_live_jail() {
    let dir = tempfile::tempdir().unwrap();
    // Use the test process's own PID — guaranteed to be alive.
    let self_pid = std::process::id();
    write_state(dir.path(), Some(self_pid), Some(self_pid));

    let decision = recover_from_run_dir(dir.path()).unwrap();
    assert!(
        matches!(
            decision,
            RecoveryDecision::LiveJail {
                jailer_pid,
                firecracker_pid,
            } if jailer_pid == self_pid && firecracker_pid == self_pid
        ),
        "expected LiveJail with self_pid={self_pid}, got {decision:?}"
    );
}

#[test]
fn mixed_live_and_dead_pid_returns_orphan() {
    let dir = tempfile::tempdir().unwrap();
    let self_pid = std::process::id();
    // One alive, one dead.
    write_state(dir.path(), Some(self_pid), Some(u32::MAX));
    write_plan(dir.path());

    let decision = recover_from_run_dir(dir.path()).unwrap();
    assert!(
        matches!(decision, RecoveryDecision::OrphanJail { .. }),
        "expected OrphanJail when one pid is dead, got {decision:?}"
    );
}

#[test]
fn orphan_without_plan_file_has_empty_reap_steps() {
    let dir = tempfile::tempdir().unwrap();
    write_state(dir.path(), Some(u32::MAX), Some(u32::MAX - 1));
    // Intentionally no plan file.

    let decision = recover_from_run_dir(dir.path()).unwrap();
    if let RecoveryDecision::OrphanJail { reap_steps } = decision {
        assert!(
            reap_steps.is_empty(),
            "no plan file → empty reap_steps, got {reap_steps:?}"
        );
    } else {
        panic!("expected OrphanJail, got {decision:?}");
    }
}
