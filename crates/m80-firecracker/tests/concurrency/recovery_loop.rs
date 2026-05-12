use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Barrier,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use m80_firecracker::{Backend, BackendConfig, CgroupMode, NetworkPolicy, SandboxConfig};
use m80_proto::{ExecRequest, ExecStatus};
use tempfile::TempDir;

use crate::common;

fn make_backend(run_root: &Path) -> Arc<Backend> {
    let config = BackendConfig {
        discovery: common::fake_discovery(run_root),
        max_concurrent_vms: 8,
        run_root: run_root.to_path_buf(),
        jail_uid: 3000,
        jail_gid: 3000,
        cgroup_mode: CgroupMode::Disabled,
    };
    Arc::new(Backend::new(config).expect("Backend::new"))
}

#[test]
fn no_background_recovery_task_is_spawned_by_backend_new() {
    let dir = TempDir::new().unwrap();
    let orphan = dir.path().join("vm-orphan");
    std::fs::create_dir_all(&orphan).unwrap();

    let _backend = make_backend(dir.path());

    assert!(orphan.exists());
}

#[test]
fn recovery_interval_is_not_an_orchestrator_constant_in_v0_1() {
    let _: fn(&Backend) -> Result<(), m80_firecracker::FcError> = Backend::recover_stale_run_root;
}

#[test]
fn recovery_is_synchronous_explicit_call() {
    let dir = TempDir::new().unwrap();
    let orphan = dir.path().join("vm-orphan");
    std::fs::create_dir_all(&orphan).unwrap();
    let backend = make_backend(dir.path());

    backend.recover_stale_run_root().unwrap();

    assert!(!orphan.exists());
}

#[test]
fn startup_recovery_is_caller_driven_before_first_admission() {
    let dir = TempDir::new().unwrap();
    let orphan = dir.path().join("vm-orphan");
    std::fs::create_dir_all(&orphan).unwrap();
    let backend = make_backend(dir.path());

    assert!(orphan.exists());
    backend.recover_stale_run_root().unwrap();
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
        Backend::new(BackendConfig {
            discovery,
            max_concurrent_vms: LAUNCHES as u32,
            run_root,
            jail_uid: 3000,
            jail_gid: 3000,
            cgroup_mode: CgroupMode::Disabled,
        })
        .expect("Backend::new"),
    );

    let stop_recovery = Arc::new(AtomicBool::new(false));
    let recovery_backend = Arc::clone(&backend);
    let recovery_stop = Arc::clone(&stop_recovery);
    let recovery = std::thread::spawn(move || {
        while !recovery_stop.load(Ordering::Relaxed) {
            recovery_backend
                .recover_stale_run_root()
                .expect("recover_stale_run_root");
            std::thread::sleep(Duration::from_millis(10));
        }
    });

    let barrier = Arc::new(Barrier::new(LAUNCHES));
    let suffix = unique_suffix() % 0x10000;
    let mut handles = Vec::new();
    for index in 0..LAUNCHES {
        let backend = Arc::clone(&backend);
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            let sandbox = backend
                .admit(SandboxConfig {
                    vm_id: Some(format!("rec-race-{suffix:04x}-{index}")),
                    workspace: None,
                    network: NetworkPolicy::NoEgress,
                    vcpu_count: Some(1),
                    mem_size_mib: Some(512),
                    boot_args: None,
                    overlay_size_bytes: 512 * 1024 * 1024,
                    idle_timeout: None,
                    daemonize: false,
                    request_id: None,
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

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos()
}
