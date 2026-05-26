//! Real-KVM coverage for stop disposition diagnostics.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::RunDirDumpGuard;
use m80_firecracker::{Backend, BackendConfig, CgroupMode, FcError, NetworkPolicy, SandboxConfig};
use m80_proto::{ExecRequest, ExecStatus};
use serde_json::Value;

// vm_id must stay under ~22 chars: the AF_UNIX socket path
// `<run_root>/<vm_id>/<fc_basename>/<vm_id>/root/firecracker.sock` is capped at
// 107 bytes by the kernel and the jail layout uses vm_id twice.

fn launch_vm(
    vm_id: &str,
    request_id: &str,
) -> (Arc<Backend>, m80_firecracker::RunningSandbox, PathBuf, u32) {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let run_root = discovery.run_root.clone();
    let backend = Arc::new(
        Backend::new(
            BackendConfig::builder(discovery)
                .max_concurrent_vms(1)
                .run_root(run_root.clone())
                .jail_uid(3000)
                .jail_gid(3000)
                .cgroup_mode(CgroupMode::Disabled)
                .build(),
        )
        .expect("Backend::new"),
    );
    let sandbox = backend
        .admit(SandboxConfig {
            vm_id: Some(vm_id.to_owned()),
            workspace: None,
            network: NetworkPolicy::NoEgress,
            vcpu_count: Some(1),
            mem_size_mib: Some(512),
            huge_pages_2m: false,
            cpuset_cpus: None,
            cpu_template: None,
            fc_log_level: None,
            drive_cache_type: None,
            boot_args: None,
            overlay_size_bytes: 512 * 1024 * 1024,
            overlay_clone_mode: Default::default(),
            idle_timeout: None,
            max_lifetime: None,
            daemonize: false,
            request_id: Some(request_id.to_owned()),
            pmem_layers: Vec::new(),
            preallocated_drive_slots: 0,
            one_shot: false,
        })
        .expect("admit");
    let running = sandbox.launch().expect("launch");
    let run_dir = running.run_dir().to_path_buf();
    let firecracker_pid = firecracker_pid(&run_dir);
    assert!(
        process_exists(firecracker_pid),
        "firecracker pid {firecracker_pid} should be alive after launch"
    );
    (backend, running, run_root.join(vm_id), firecracker_pid)
}

#[test]
#[ignore = "requires-kvm requires-artifacts"]
fn stop_disposition_normal_records_normal_stop() {
    let request_id = "req-stop-normal";
    let vm_id = common::unique_vm_id("stop-normal");
    let (_backend, running, run_dir, firecracker_pid) = launch_vm(&vm_id, request_id);
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());

    let stopped = running.stop().expect("stop");

    wait_dead(firecracker_pid);
    assert_exit_reason(&run_dir, request_id, "stop complete", "normal_stop");

    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires-kvm requires-artifacts"]
fn stop_disposition_force_records_force_kill() {
    let request_id = "req-stop-force";
    let vm_id = common::unique_vm_id("stop-force");
    let (_backend, running, run_dir, firecracker_pid) = launch_vm(&vm_id, request_id);
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());

    let stopped = running.force_kill().expect("force kill");

    wait_dead(firecracker_pid);
    assert_exit_reason(&run_dir, request_id, "force kill complete", "force_kill");

    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires-kvm requires-artifacts"]
fn implicit_drop_force_kills_before_resource_teardown() {
    let request_id = "req-drop-order";
    let vm_id = common::unique_vm_id("drop-order");
    let (backend, running, run_dir, firecracker_pid) = launch_vm(&vm_id, request_id);
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());

    drop(running);

    wait_dead(firecracker_pid);
    assert_no_mountinfo_references(&run_dir);
    backend
        .recover_stale_run_root(true)
        .expect("recover run-dir after implicit drop");
    assert!(
        !run_dir.exists(),
        "stale recovery must remove dropped sandbox run-dir: {}",
        run_dir.display()
    );
}

#[test]
#[ignore = "requires-kvm requires-artifacts"]
fn forced_kill_ambiguous_blocks_release() {
    let request_id = "req-force-kill-ambiguous";
    let vm_id = common::unique_vm_id("fk-ambig");
    let (backend, running, run_dir, firecracker_pid) = launch_vm(&vm_id, request_id);
    let jailer_pid = jailer_pid(&run_dir);
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());
    let fault = EnvGuard::set(
        "M80_TEST_FORCE_KILL_EPERM_FOR_PID",
        &firecracker_pid.to_string(),
    );

    let err = match running.force_kill() {
        Ok(stopped) => {
            stopped.delete().expect("delete unexpected stopped sandbox");
            panic!("forced-kill EPERM injection unexpectedly succeeded");
        }
        Err(err) => err,
    };
    let diagnostics = read_diagnostics(&run_dir);
    let blocked_admission = backend
        .admit(SandboxConfig {
            vm_id: Some(common::unique_vm_id("fk-ambig-r")),
            workspace: None,
            network: NetworkPolicy::NoEgress,
            vcpu_count: Some(1),
            mem_size_mib: Some(512),
            huge_pages_2m: false,
            cpuset_cpus: None,
            cpu_template: None,
            fc_log_level: None,
            drive_cache_type: None,
            boot_args: None,
            overlay_size_bytes: 512 * 1024 * 1024,
            overlay_clone_mode: Default::default(),
            idle_timeout: None,
            max_lifetime: None,
            daemonize: false,
            request_id: None,
            pmem_layers: Vec::new(),
            preallocated_drive_slots: 0,
            one_shot: false,
        })
        .expect_err("ambiguous force kill must keep the admission permit held");

    drop(fault);
    kill_pid_best_effort(firecracker_pid);
    if jailer_pid != 0 && jailer_pid != firecracker_pid {
        kill_pid_best_effort(jailer_pid);
    }
    let _ = std::fs::remove_file(run_dir.join(m80_firecracker::OWNERSHIP_LOCK));
    backend
        .recover_stale_run_root(true)
        .expect("recover ambiguous force-kill residue");
    assert!(
        !run_dir.exists(),
        "ambiguous force-kill harness cleanup must remove run-dir: {}",
        run_dir.display()
    );

    assert!(
        matches!(err, FcError::KillFailed { ref source, .. } if source.kind() == std::io::ErrorKind::PermissionDenied),
        "expected injected EPERM/PermissionDenied, got {err:?}"
    );
    assert!(matches!(
        blocked_admission,
        FcError::AdmissionRefused { .. }
    ));
    assert_diagnostics_message_contains(&diagnostics, "ForcedKillAmbiguous");
}

#[test]
#[ignore = "requires-kvm requires-artifacts"]
fn stop_with_unreachable_guestd_still_returns_stopped_and_releases_after_delete() {
    let request_id = "req-stop-unreachable-guestd";
    let vm_id = common::unique_vm_id("stop-unreach");
    let (backend, mut running, run_dir, firecracker_pid) = launch_vm(&vm_id, request_id);
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());

    exec_sh(
        &mut running,
        "(sleep 0.2; kill -STOP 1) >/dev/null 2>&1 & printf armed",
    );
    std::thread::sleep(Duration::from_millis(500));

    let stopped = running.stop().expect("stop with unreachable guestd");

    wait_dead(firecracker_pid);
    assert_exit_reason(&run_dir, request_id, "stop complete", "force_kill");

    stopped.delete().expect("delete after failed shutdown RPC");
    assert!(
        !run_dir.exists(),
        "delete must remove run-dir after failed shutdown RPC: {}",
        run_dir.display()
    );
    let admitted = backend
        .admit(SandboxConfig {
            vm_id: Some(common::unique_vm_id("stop-unreach-r")),
            workspace: None,
            network: NetworkPolicy::NoEgress,
            vcpu_count: Some(1),
            mem_size_mib: Some(512),
            huge_pages_2m: false,
            cpuset_cpus: None,
            cpu_template: None,
            fc_log_level: None,
            drive_cache_type: None,
            boot_args: None,
            overlay_size_bytes: 512 * 1024 * 1024,
            overlay_clone_mode: Default::default(),
            idle_timeout: None,
            max_lifetime: None,
            daemonize: false,
            request_id: None,
            pmem_layers: Vec::new(),
            preallocated_drive_slots: 0,
            one_shot: false,
        })
        .expect("admission permit must be released after delete");
    drop(admitted);
}

fn firecracker_pid(run_dir: &Path) -> u32 {
    jailer_state_pid(run_dir, "firecracker_pid")
}

fn jailer_pid(run_dir: &Path) -> u32 {
    jailer_state_pid(run_dir, "jailer_pid")
}

fn jailer_state_pid(run_dir: &Path, key: &str) -> u32 {
    let state_path = run_dir.join("jailer-state.json");
    let state: Value = serde_json::from_str(
        &std::fs::read_to_string(&state_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", state_path.display())),
    )
    .expect("jailer-state.json parses");
    state[key]
        .as_u64()
        .unwrap_or_else(|| panic!("{key} missing from {}", state_path.display())) as u32
}

fn process_exists(pid: u32) -> bool {
    match nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None) {
        Ok(()) => true,
        Err(nix::errno::Errno::ESRCH) => false,
        Err(e) => panic!("probe pid {pid}: {e}"),
    }
}

fn wait_dead(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if !process_exists(pid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(
        !process_exists(pid),
        "firecracker pid {pid} should be gone after stop disposition"
    );
}

fn assert_no_mountinfo_references(path: &Path) {
    let mountinfo = std::fs::read_to_string("/proc/self/mountinfo").expect("host mountinfo");
    let needle = path.as_os_str().as_encoded_bytes();
    assert!(
        !mountinfo
            .as_bytes()
            .windows(needle.len())
            .any(|w| w == needle),
        "host mountinfo still references {} after implicit drop:\n{mountinfo}",
        path.display()
    );
}

fn kill_pid_best_effort(pid: u32) {
    if pid == 0 {
        return;
    }
    let _ = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pid as i32),
        nix::sys::signal::Signal::SIGKILL,
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if !process_exists(pid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn kill_pid_best_effort_ignores_no_live_jailer_sentinel() {
    kill_pid_best_effort(0);
}

fn exec_sh(running: &mut m80_firecracker::RunningSandbox, script: &str) {
    let response = running
        .exec(ExecRequest {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(10_000),
            streaming: false,
        })
        .unwrap_or_else(|e| panic!("exec {script:?}: {e}"));
    assert_eq!(response.status, ExecStatus::Completed);
    assert_eq!(
        response.exit_code,
        Some(0),
        "exec {script:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&response.stdout),
        String::from_utf8_lossy(&response.stderr)
    );
}

fn read_diagnostics(run_dir: &Path) -> Vec<Value> {
    let path = run_dir.join("diagnostics.jsonl");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
        .lines()
        .map(|line| serde_json::from_str(line).expect("diagnostics JSONL line parses"))
        .collect()
}

fn assert_exit_reason(run_dir: &Path, request_id: &str, message: &str, reason: &str) {
    let events = read_diagnostics(run_dir);
    assert!(
        events.iter().any(|event| {
            event["event_kind"] == "lifecycle"
                && event["phase"] == "Stop"
                && event["request_id"] == request_id
                && event["message"] == message
                && event["exit_reason"]["reason"] == reason
        }),
        "missing stop disposition message={message:?} reason={reason:?}; events={events:#?}"
    );
}

fn assert_diagnostics_message_contains(events: &[Value], needle: &str) {
    assert!(
        events.iter().any(|event| {
            event["event_kind"] == "lifecycle"
                && event["phase"] == "Stop"
                && event["message"]
                    .as_str()
                    .is_some_and(|message| message.contains(needle))
        }),
        "missing diagnostics message containing {needle:?}; events={events:#?}"
    );
}

struct EnvGuard {
    key: &'static str,
    old: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let old = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, old }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.old {
            Some(old) => std::env::set_var(self.key, old),
            None => std::env::remove_var(self.key),
        }
    }
}
