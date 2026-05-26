//! Tests for `SandboxConfig::max_lifetime` and absolute lifecycle expiry.
//!
//! Unit tests cover the public default and typed error. KVM-gated tests
//! exercise the actual launch watcher behavior.
//!
//! Behavior doc: `docs/behaviors/lifecycle/max-lifetime.md`.

mod common;

use std::time::{Duration, Instant};

use m80_firecracker::{FcError, SandboxConfig};

#[test]
fn max_lifetime_default_is_none() {
    let cfg = SandboxConfig::default();
    assert_eq!(cfg.max_lifetime, None);
}

#[test]
fn lifetime_expired_error_displays_limit() {
    let err = FcError::LifetimeExpired {
        limit: Duration::from_secs(7),
    };
    let text = err.to_string();
    assert!(text.contains("max lifetime"));
    assert!(text.contains("7s"));
}

#[test]
#[ignore = "requires-kvm requires-artifacts"]
fn max_lifetime_fires_after_configured_duration_with_no_exec_in_flight() {
    let mut sandbox = launch_with(Some(Duration::from_secs(1)), None);
    let run_dir = sandbox.run_dir().to_owned();
    let _dump_guard = common::RunDirDumpGuard::new(run_dir.clone());
    let firecracker_pid = firecracker_pid(&run_dir);

    std::thread::sleep(Duration::from_millis(1500));

    let result = sandbox.exec(true_request());
    cleanup_running(sandbox);
    let err = result.expect_err("exec after max_lifetime must fail");
    assert!(matches!(err, FcError::LifetimeExpired { .. }));
    wait_for_process_exit(firecracker_pid, Duration::from_secs(5));
}

#[test]
#[ignore = "requires-kvm requires-artifacts"]
fn max_lifetime_expiring_during_exec_allows_in_flight_result_then_rejects_next_exec() {
    let mut sandbox = launch_with(Some(Duration::from_secs(1)), None);
    let run_dir = sandbox.run_dir().to_owned();
    let _dump_guard = common::RunDirDumpGuard::new(run_dir);

    let response = sandbox
        .exec(m80_proto::ExecRequest {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "sleep 2; echo done".into()],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        })
        .expect("in-flight exec should finish even when lifetime expires");
    assert_eq!(String::from_utf8_lossy(&response.stdout), "done\n");

    let result = sandbox.exec(true_request());
    cleanup_running(sandbox);
    let err = result.expect_err("next exec after max_lifetime must fail");
    assert!(matches!(err, FcError::LifetimeExpired { .. }));
}

#[test]
#[ignore = "requires-kvm requires-artifacts"]
fn max_lifetime_none_never_fires() {
    let mut sandbox = launch_with(None, None);
    let run_dir = sandbox.run_dir().to_owned();
    let _dump_guard = common::RunDirDumpGuard::new(run_dir);

    std::thread::sleep(Duration::from_millis(1500));

    sandbox
        .exec(true_request())
        .expect("max_lifetime=None must not expire the sandbox");
    sandbox.stop().expect("stop").delete().expect("delete");
}

#[test]
#[ignore = "requires-kvm requires-artifacts"]
fn idle_timeout_wins_when_shorter_than_max_lifetime() {
    let mut sandbox = launch_with(Some(Duration::from_secs(60)), Some(Duration::from_secs(1)));
    let run_dir = sandbox.run_dir().to_owned();
    let _dump_guard = common::RunDirDumpGuard::new(run_dir);

    std::thread::sleep(Duration::from_millis(1500));

    let result = sandbox.exec(true_request());
    cleanup_running(sandbox);
    let err = result.expect_err("shorter idle timeout must fire first");
    assert!(matches!(err, FcError::IdleTimedOut));
}

#[test]
#[ignore = "requires-kvm requires-artifacts"]
fn max_lifetime_wins_when_shorter_than_idle_timeout() {
    let mut sandbox = launch_with(Some(Duration::from_secs(1)), Some(Duration::from_secs(60)));
    let run_dir = sandbox.run_dir().to_owned();
    let _dump_guard = common::RunDirDumpGuard::new(run_dir);

    std::thread::sleep(Duration::from_millis(1500));

    let result = sandbox.exec(true_request());
    cleanup_running(sandbox);
    let err = result.expect_err("shorter max_lifetime must fire first");
    assert!(matches!(err, FcError::LifetimeExpired { .. }));
}

fn launch_with(
    max_lifetime: Option<Duration>,
    idle_timeout: Option<Duration>,
) -> m80_firecracker::RunningSandbox {
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
        idle_timeout,
        max_lifetime,
        request_id: None,
        ..SandboxConfig::default()
    };
    backend.admit(cfg).expect("admit").launch().expect("launch")
}

fn true_request() -> m80_proto::ExecRequest {
    m80_proto::ExecRequest {
        program: "/bin/true".into(),
        args: vec![],
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(5_000),
        streaming: false,
    }
}

fn cleanup_running(sandbox: m80_firecracker::RunningSandbox) {
    sandbox
        .force_kill()
        .expect("force kill expired sandbox for cleanup")
        .delete()
        .expect("delete expired sandbox run dir");
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
