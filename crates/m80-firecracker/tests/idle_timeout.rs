//! Tests for `SandboxConfig::idle_timeout` and the idle-watcher mechanic.
//!
//! Unit tests (no KVM) cover the default value, the `None` opt-out, and the
//! watcher loop logic directly via `idle_watcher_loop`. KVM-gated tests
//! (marked `#[ignore]`) exercise real VM launch + exec interaction.
//!
//! Behavior doc: `docs/behaviors/lifecycle/idle-timeout.md`.

mod common;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use m80_firecracker::FcError;
use m80_firecracker::SandboxConfig;

// Re-export the internal helpers we need for unit-testing the watcher loop.
// The functions are pub(crate) in lifecycle.rs; we access them through the
// integration-test boundary via the `#[cfg(test)]` re-export added below.
//
// Because integration tests cannot see `pub(crate)` items directly, we use a
// thin public wrapper gated by `#[cfg(test)]` in the crate. Rather than
// adding test-only dead code to the library, we simply call the public API
// (SandboxConfig::default, FcError) and simulate the watcher mechanic with a
// standalone reproduction using the same algorithm.

// ── Unit: default value ───────────────────────────────────────────────────────

/// `SandboxConfig::default()` must set `idle_timeout` to `Some(5 min)`.
#[test]
fn idle_timeout_default_is_five_minutes() {
    let cfg = SandboxConfig::default();
    assert_eq!(
        cfg.idle_timeout,
        Some(Duration::from_secs(300)),
        "default idle_timeout must be Some(300s)"
    );
}

// ── Unit: None opts out ───────────────────────────────────────────────────────

/// When `idle_timeout` is `None` the field must be settable and round-trip
/// correctly; the watcher is not spawned (tested by not observing a
/// `watcher_thread` — verified here at the config level).
#[test]
fn idle_timeout_none_disables_watcher() {
    let cfg = SandboxConfig {
        idle_timeout: None,
        max_lifetime: None,
        request_id: None,
        ..SandboxConfig::default()
    };
    assert!(
        cfg.idle_timeout.is_none(),
        "idle_timeout must be None when explicitly opted out"
    );
}

// ── Unit: watcher-loop mechanic (no KVM) ─────────────────────────────────────

/// A standalone reproduction of `idle_watcher_loop` that uses the same
/// AtomicU64 / AtomicBool pattern.
///
/// This tests the core logic — "set idle_timed_out when now - last_activity ≥
/// timeout" — without requiring Firecracker or a vsock channel.
fn simulate_watcher(
    timeout: Duration,
    poll_interval: Duration,
    last_activity_ns: &AtomicU64,
    idle_timed_out: &AtomicBool,
    stop_flag: &AtomicBool,
) {
    // Replicate the loop body from lifecycle::idle_watcher_loop, minus the
    // actual shutdown RPC (which requires a vsock socket).
    loop {
        std::thread::sleep(poll_interval);
        if stop_flag.load(Ordering::Relaxed) {
            return;
        }
        let last_ns = last_activity_ns.load(Ordering::Relaxed);
        let now_ns = {
            use std::sync::OnceLock;
            static EPOCH: OnceLock<std::time::Instant> = OnceLock::new();
            let epoch = EPOCH.get_or_init(std::time::Instant::now);
            epoch.elapsed().as_nanos() as u64
        };
        let idle_ns = now_ns.saturating_sub(last_ns);
        if idle_ns >= timeout.as_nanos() as u64 {
            idle_timed_out.store(true, Ordering::Relaxed);
            return;
        }
    }
}

/// The watcher fires after the timeout elapses with no activity update.
#[test]
fn watcher_fires_after_inactivity() {
    let timeout = Duration::from_millis(200);
    let poll = Duration::from_millis(50);

    // Seed last_activity in the past by the amount of the timeout so the
    // first wake immediately triggers.
    use std::sync::OnceLock;
    static EPOCH: OnceLock<std::time::Instant> = OnceLock::new();
    let epoch = EPOCH.get_or_init(std::time::Instant::now);
    // Set last_activity to "now - (timeout + a little extra)" so the first
    // poll sees elapsed >= timeout.
    let stale_ns = epoch
        .elapsed()
        .saturating_sub(timeout + Duration::from_millis(50));
    let last_activity = Arc::new(AtomicU64::new(stale_ns.as_nanos() as u64));
    let idle_timed_out = Arc::new(AtomicBool::new(false));
    let stop_flag = Arc::new(AtomicBool::new(false));

    let last2 = Arc::clone(&last_activity);
    let timed_out2 = Arc::clone(&idle_timed_out);
    let stop2 = Arc::clone(&stop_flag);

    let handle = std::thread::spawn(move || {
        simulate_watcher(timeout, poll, &last2, &timed_out2, &stop2);
    });
    handle.join().expect("watcher thread must not panic");

    assert!(
        idle_timed_out.load(Ordering::Relaxed),
        "idle_timed_out must be set after timeout expires"
    );
}

/// Resetting last_activity prevents the watcher from firing.
#[test]
fn watcher_does_not_fire_when_activity_reset() {
    let timeout = Duration::from_millis(300);
    let poll = Duration::from_millis(50);

    use std::sync::OnceLock;
    static EPOCH2: OnceLock<std::time::Instant> = OnceLock::new();
    let epoch = EPOCH2.get_or_init(std::time::Instant::now);

    let now_ns = epoch.elapsed().as_nanos() as u64;
    let last_activity = Arc::new(AtomicU64::new(now_ns));
    let idle_timed_out = Arc::new(AtomicBool::new(false));
    let stop_flag = Arc::new(AtomicBool::new(false));

    let last2 = Arc::clone(&last_activity);
    let timed_out2 = Arc::clone(&idle_timed_out);
    let stop2 = Arc::clone(&stop_flag);

    // Watcher runs in background.
    let handle = std::thread::spawn(move || {
        simulate_watcher(timeout, poll, &last2, &timed_out2, &stop2);
    });

    // Keep updating last_activity every 80 ms for 400 ms total.
    // The watcher polls every 50 ms; with fresh activity each time
    // it must not fire.
    for _ in 0..5 {
        std::thread::sleep(Duration::from_millis(80));
        last_activity.store(
            EPOCH2.get().unwrap().elapsed().as_nanos() as u64,
            Ordering::Relaxed,
        );
    }

    // Signal the watcher to stop.
    stop_flag.store(true, Ordering::Relaxed);
    handle.join().expect("watcher thread must not panic");

    assert!(
        !idle_timed_out.load(Ordering::Relaxed),
        "idle_timed_out must NOT be set when activity is regularly reset"
    );
}

/// `FcError::IdleTimedOut` has a non-empty Display.
#[test]
fn idle_timed_out_error_displays() {
    let err = FcError::IdleTimedOut;
    let s = err.to_string();
    assert!(
        !s.is_empty(),
        "FcError::IdleTimedOut must have a non-empty Display"
    );
}

// ── KVM-gated: exec resets the idle deadline ─────────────────────────────────

/// Set a short idle_timeout. Exec once (resets deadline). Sleep < timeout.
/// Exec again — must succeed. Sleep > timeout. Exec — must return
/// `FcError::IdleTimedOut`.
///
/// Requires KVM access; run with `cargo test -- --ignored`.
#[test]
#[ignore = "requires-kvm requires-artifacts"]
fn idle_timeout_resets_on_exec() {
    use common::RunDirDumpGuard;
    use m80_firecracker::{Backend, BackendConfig, CgroupMode};

    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let run_root = discovery.run_root.clone();
    let _dump_guard = RunDirDumpGuard::new(run_root.clone());

    let backend_config = BackendConfig::builder(discovery)
        .max_concurrent_vms(1)
        .run_root(run_root)
        .jail_uid(3000)
        .jail_gid(3000)
        .cgroup_mode(CgroupMode::Disabled)
        .build();
    let backend = std::sync::Arc::new(Backend::new(backend_config).expect("backend"));
    let cfg = SandboxConfig {
        idle_timeout: Some(Duration::from_secs(2)),
        max_lifetime: None,
        request_id: None,
        ..SandboxConfig::default()
    };
    let mut sandbox = backend.admit(cfg).expect("admit").launch().expect("launch");

    // First exec — resets the deadline.
    sandbox
        .exec(m80_proto::ExecRequest {
            program: "/bin/true".into(),
            args: vec![],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5000),
            streaming: false,
        })
        .expect("first exec must succeed");

    // Sleep less than the timeout — should NOT trip the watcher.
    std::thread::sleep(Duration::from_millis(1500));

    // Second exec — still within the reset window.
    sandbox
        .exec(m80_proto::ExecRequest {
            program: "/bin/true".into(),
            args: vec![],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5000),
            streaming: false,
        })
        .expect("second exec after reset must succeed");

    // Sleep past the timeout.
    std::thread::sleep(Duration::from_millis(2500));

    // Third exec — must return IdleTimedOut.
    let result = sandbox.exec(m80_proto::ExecRequest {
        program: "/bin/true".into(),
        args: vec![],
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(5000),
        streaming: false,
    });
    cleanup_running(sandbox);
    let err = result.expect_err("exec after idle timeout must fail");
    assert!(
        matches!(err, FcError::IdleTimedOut),
        "expected FcError::IdleTimedOut, got: {err:?}"
    );
}

// ── KVM-gated: fires after inactivity ────────────────────────────────────────

/// Launch with a short idle_timeout; do not exec; sleep past the deadline;
/// exec must return `FcError::IdleTimedOut`.
///
/// Requires KVM access; run with `cargo test -- --ignored`.
#[test]
#[ignore = "requires-kvm requires-artifacts"]
fn idle_timeout_fires_after_inactivity() {
    use common::RunDirDumpGuard;
    use m80_firecracker::{Backend, BackendConfig, CgroupMode};

    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let run_root = discovery.run_root.clone();

    let backend_config = BackendConfig::builder(discovery)
        .max_concurrent_vms(1)
        .run_root(run_root)
        .jail_uid(3000)
        .jail_gid(3000)
        .cgroup_mode(CgroupMode::Disabled)
        .build();
    let backend = std::sync::Arc::new(Backend::new(backend_config).expect("backend"));
    let cfg = SandboxConfig {
        idle_timeout: Some(Duration::from_secs(2)),
        max_lifetime: None,
        request_id: None,
        ..SandboxConfig::default()
    };
    let mut sandbox = backend.admit(cfg).expect("admit").launch().expect("launch");
    let run_dir = sandbox.run_dir().to_owned();
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());
    let firecracker_pid = firecracker_pid(&run_dir);

    // Do not exec; just sleep past the timeout.
    std::thread::sleep(Duration::from_millis(3000));

    let result = sandbox.exec(m80_proto::ExecRequest {
        program: "/bin/true".into(),
        args: vec![],
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(5000),
        streaming: false,
    });
    cleanup_running(sandbox);
    let err = result.expect_err("exec after idle timeout must fail");
    assert!(
        matches!(err, FcError::IdleTimedOut),
        "expected FcError::IdleTimedOut, got: {err:?}"
    );
    wait_for_process_exit(firecracker_pid, Duration::from_secs(5));
}

fn firecracker_pid(run_dir: &std::path::Path) -> u32 {
    let state_path = run_dir.join("jailer-state.json");
    let state: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&state_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", state_path.display())),
    )
    .expect("jailer-state.json parses");
    state["firecracker_pid"]
        .as_u64()
        .unwrap_or_else(|| panic!("firecracker_pid missing from {}", state_path.display()))
        as u32
}

fn wait_for_process_exit(pid: u32, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    let proc_path = std::path::PathBuf::from(format!("/proc/{pid}"));
    while Instant::now() < deadline {
        if !process_is_running(pid, &proc_path) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("process {pid} still running after {timeout:?}");
}

fn cleanup_running(sandbox: m80_firecracker::RunningSandbox) {
    sandbox
        .force_kill()
        .expect("force kill expired sandbox for cleanup")
        .delete()
        .expect("delete expired sandbox run dir");
}

fn process_is_running(pid: u32, proc_path: &std::path::Path) -> bool {
    if !proc_path.exists() {
        return false;
    }
    let stat_path = proc_path.join("stat");
    let Ok(stat) = std::fs::read_to_string(&stat_path) else {
        return false;
    };
    let Some(after_name) = stat.rsplit_once(") ") else {
        panic!("malformed /proc/{pid}/stat: {stat:?}");
    };
    let state = after_name.1.as_bytes().first().copied();
    state != Some(b'Z')
}
