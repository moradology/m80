//! Real-KVM coverage for stop disposition diagnostics.

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::RunDirDumpGuard;
use m80_firecracker::{Backend, BackendConfig, CgroupMode, NetworkPolicy, SandboxConfig};
use serde_json::Value;

fn unique_vm_id(prefix: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis()
        % 1_000_000;
    format!("{prefix}-{millis}")
}

fn launch_vm(vm_id: &str, request_id: &str) -> (m80_firecracker::RunningSandbox, PathBuf, u32) {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let run_root = discovery.run_root.clone();
    let backend = std::sync::Arc::new(
        Backend::new(BackendConfig {
            discovery,
            max_concurrent_vms: 1,
            run_root: run_root.clone(),
            jail_uid: 3000,
            jail_gid: 3000,
            cgroup_mode: CgroupMode::Disabled,
        })
        .expect("Backend::new"),
    );
    let sandbox = backend
        .admit(SandboxConfig {
            vm_id: Some(vm_id.to_owned()),
            workspace: None,
            network: NetworkPolicy::NoEgress,
            vcpu_count: Some(1),
            mem_size_mib: Some(512),
            boot_args: None,
            overlay_size_bytes: 512 * 1024 * 1024,
            idle_timeout: None,
            daemonize: false,
            request_id: Some(request_id.to_owned()),
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
    (running, run_root.join(vm_id), firecracker_pid)
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn stop_disposition_normal_records_normal_stop() {
    let request_id = "req-stop-normal";
    let vm_id = unique_vm_id("stop-normal");
    let (running, run_dir, firecracker_pid) = launch_vm(&vm_id, request_id);
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());

    let stopped = running.stop().expect("stop");

    wait_dead(firecracker_pid);
    assert_exit_reason(&run_dir, request_id, "stop complete", "normal_stop");

    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn stop_disposition_force_records_force_kill() {
    let request_id = "req-stop-force";
    let vm_id = unique_vm_id("stop-force");
    let (running, run_dir, firecracker_pid) = launch_vm(&vm_id, request_id);
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());

    let stopped = running.force_kill().expect("force kill");

    wait_dead(firecracker_pid);
    assert_exit_reason(&run_dir, request_id, "force kill complete", "force_kill");

    stopped.delete().expect("delete");
}

fn firecracker_pid(run_dir: &Path) -> u32 {
    let state_path = run_dir.join("jailer-state.json");
    let state: Value = serde_json::from_str(
        &std::fs::read_to_string(&state_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", state_path.display())),
    )
    .expect("jailer-state.json parses");
    state["firecracker_pid"]
        .as_u64()
        .unwrap_or_else(|| panic!("firecracker_pid missing from {}", state_path.display()))
        as u32
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
