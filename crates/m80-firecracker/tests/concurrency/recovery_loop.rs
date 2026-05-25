use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Barrier,
};
use std::time::Duration;

use m80_firecracker::{Backend, BackendConfig, CgroupMode, NetworkPolicy, SandboxConfig};
use m80_proto::{ExecRequest, ExecStatus};
use tempfile::TempDir;

use crate::common;

#[test]
fn backend_new_runs_one_startup_recovery_pass() {
    let dir = TempDir::new().unwrap();
    let orphan = dir.path().join("vm-orphan");
    std::fs::create_dir_all(&orphan).unwrap();

    let _backend = common::make_fake_backend(8, dir.path());

    assert!(!orphan.exists());
}

#[test]
fn recovery_interval_is_not_an_orchestrator_constant_in_v0_1() {
    let _: fn(&Backend, bool) -> Result<(), m80_firecracker::FcError> =
        Backend::recover_stale_run_root;
}

#[test]
fn recovery_is_synchronous_explicit_call() {
    let dir = TempDir::new().unwrap();
    let orphan = dir.path().join("vm-orphan");
    std::fs::create_dir_all(&orphan).unwrap();
    let backend = common::make_fake_backend(8, dir.path());

    backend.recover_stale_run_root(false).unwrap();

    assert!(!orphan.exists());
}

#[test]
fn explicit_recovery_api_remains_available_after_startup_pass() {
    let dir = TempDir::new().unwrap();
    let backend = common::make_fake_backend(8, dir.path());
    let orphan = dir.path().join("vm-orphan-after-startup");
    std::fs::create_dir_all(&orphan).unwrap();

    assert!(orphan.exists());
    backend.recover_stale_run_root(false).unwrap();
    assert!(!orphan.exists());
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn recovery_during_launch_preserves_fresh_vms() {
    const LAUNCHES: usize = 10;

    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let run_root = discovery.run_root.clone();
    let backend = Arc::new(
        Backend::new(
            BackendConfig::builder(discovery)
                .max_concurrent_vms(LAUNCHES as u32)
                .run_root(run_root)
                .jail_uid(3000)
                .jail_gid(3000)
                .cgroup_mode(CgroupMode::Disabled)
                .build(),
        )
        .expect("Backend::new"),
    );

    let stop_recovery = Arc::new(AtomicBool::new(false));
    let recovery_backend = Arc::clone(&backend);
    let recovery_stop = Arc::clone(&stop_recovery);
    let recovery = std::thread::spawn(move || {
        while !recovery_stop.load(Ordering::Relaxed) {
            recovery_backend
                .recover_stale_run_root(false)
                .expect("recover_stale_run_root");
            std::thread::sleep(Duration::from_millis(10));
        }
    });

    let barrier = Arc::new(Barrier::new(LAUNCHES));
    let run_prefix = common::unique_vm_id("rec-race");
    let mut handles = Vec::new();
    for index in 0..LAUNCHES {
        let backend = Arc::clone(&backend);
        let barrier = Arc::clone(&barrier);
        let run_prefix = run_prefix.clone();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            let sandbox = backend
                .admit(SandboxConfig {
                    vm_id: Some(format!("{run_prefix}-{index}")),
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
                .expect("admit");
            let mut running = sandbox.launch().expect("launch");
            let response = running.exec(true_request()).expect("exec true");
            assert_eq!(response.status, ExecStatus::Completed);
            assert_eq!(response.exit_code, Some(0));
            let stopped = running.stop().expect("stop");
            stopped.delete().expect("delete");
        }));
    }

    let mut launch_worker_panicked = false;
    for handle in handles {
        if handle.join().is_err() {
            launch_worker_panicked = true;
        }
    }
    stop_recovery.store(true, Ordering::Relaxed);
    recovery.join().expect("recovery worker panicked");
    assert!(!launch_worker_panicked, "launch worker panicked");
}

fn true_request() -> ExecRequest {
    ExecRequest {
        program: "/bin/true".into(),
        args: Vec::new(),
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(5_000),
        streaming: false,
    }
}
