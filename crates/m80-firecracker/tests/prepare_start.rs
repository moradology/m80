//! Real-KVM coverage for the cold prepare/start lifecycle split.

mod common;

use std::sync::Arc;
use std::time::Duration;

use m80_firecracker::{Backend, BackendConfig, CgroupMode, NetworkPolicy, SandboxConfig};
use m80_proto::{ExecRequest, ExecStatus};

use common::RunDirDumpGuard;

fn backend_with_temp_run_root() -> (Arc<Backend>, tempfile::TempDir) {
    let discovery = m80_preflight::run().expect("preflight");
    let run_root = tempfile::Builder::new()
        .prefix("m80ps-")
        .tempdir_in("/tank/tmp")
        .or_else(|_| tempfile::Builder::new().prefix("m80ps-").tempdir())
        .expect("run root");
    let config = BackendConfig::builder(discovery)
        .max_concurrent_vms(1)
        .run_root(run_root.path())
        .jail_uid(3000)
        .jail_gid(3000)
        .cgroup_mode(CgroupMode::Disabled)
        .build();
    (
        Arc::new(Backend::new(config).expect("Backend::new")),
        run_root,
    )
}

fn sandbox_config(vm_id_prefix: &str) -> SandboxConfig {
    SandboxConfig {
        vm_id: Some(common::unique_vm_id(vm_id_prefix)),
        workspace: None,
        network: NetworkPolicy::NoEgress,
        vcpu_count: Some(1),
        mem_size_mib: Some(512),
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
    }
}

fn exec_true(running: &mut m80_firecracker::RunningSandbox) {
    let response = running
        .exec(ExecRequest {
            program: "/bin/true".into(),
            args: Vec::new(),
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        })
        .expect("exec /bin/true");
    assert_eq!(response.status, ExecStatus::Completed);
    assert_eq!(response.exit_code, Some(0));
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn prepare_start_yields_functional_running_sandbox() {
    let (backend, _run_root) = backend_with_temp_run_root();
    let prepared = backend
        .admit(sandbox_config("ps"))
        .expect("admit")
        .prepare()
        .expect("prepare");
    let run_dir = prepared.run_dir().to_owned();
    let _dump_guard = RunDirDumpGuard::new(run_dir);

    let mut running = prepared.start().expect("start");
    exec_true(&mut running);
    running.stop().expect("stop").delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn prepared_abort_releases_permit_and_deletes_run_dir() {
    let (backend, _run_root) = backend_with_temp_run_root();
    let prepared = backend
        .admit(sandbox_config("pa"))
        .expect("admit")
        .prepare()
        .expect("prepare");
    let run_dir = prepared.run_dir().to_owned();

    prepared.abort().expect("abort");

    assert!(!run_dir.exists(), "abort must remove prepared run dir");
    let second = backend.admit(sandbox_config("pa2"));
    assert!(
        second.is_ok(),
        "abort must release the admission permit for another VM: {second:?}"
    );
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn prepared_sandbox_can_start_after_delay() {
    let (backend, _run_root) = backend_with_temp_run_root();
    let prepared = backend
        .admit(sandbox_config("pd"))
        .expect("admit")
        .prepare()
        .expect("prepare");
    let run_dir = prepared.run_dir().to_owned();
    let _dump_guard = RunDirDumpGuard::new(run_dir);

    std::thread::sleep(Duration::from_millis(500));
    let mut running = prepared.start().expect("start after delay");
    exec_true(&mut running);
    running.stop().expect("stop").delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn launch_convenience_path_still_yields_functional_running_sandbox() {
    let (backend, _run_root) = backend_with_temp_run_root();
    let mut running = backend
        .admit(sandbox_config("pl"))
        .expect("admit")
        .launch()
        .expect("launch");
    let run_dir = running.run_dir().to_owned();
    let _dump_guard = RunDirDumpGuard::new(run_dir);

    exec_true(&mut running);
    running.stop().expect("stop").delete().expect("delete");
}
