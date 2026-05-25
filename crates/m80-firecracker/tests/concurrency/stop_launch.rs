use std::sync::{mpsc, Arc, Barrier};

use m80_firecracker::{Backend, BackendConfig, CgroupMode, NetworkPolicy, SandboxConfig};
use m80_proto::{ExecRequest, ExecStatus};

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn concurrent_stop_launch_run_dir_race() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let run_root = discovery.run_root.clone();
    let backend = Arc::new(
        Backend::new(
            BackendConfig::builder(discovery)
                .max_concurrent_vms(2)
                .run_root(run_root)
                .jail_uid(3000)
                .jail_gid(3000)
                .cgroup_mode(CgroupMode::Disabled)
                .build(),
        )
        .expect("Backend::new"),
    );

    // vm_id stays short: AF_UNIX path budget is 107 bytes and m80-jailer's
    // nested layout uses vm_id twice.
    let vm_id = format!("slr-{:04x}", crate::common::unique_suffix() % 0x10000);
    let sandbox = backend.admit(config(&vm_id)).expect("admit first sandbox");
    let mut running = sandbox.launch().expect("launch first sandbox");
    let response = running.exec(true_request()).expect("exec true");
    assert_eq!(response.status, ExecStatus::Completed);
    assert_eq!(response.exit_code, Some(0));

    let run_dir = running.run_dir().to_path_buf();
    assert!(
        run_dir.join(m80_firecracker::OWNERSHIP_LOCK).exists(),
        "ownership.lock must be held while the VM is running"
    );

    let barrier = Arc::new(Barrier::new(2));
    let stop_barrier = Arc::clone(&barrier);
    let (stopped_tx, stopped_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let stopper = std::thread::spawn(move || {
        stop_barrier.wait();
        let stopped = running.stop().expect("stop first sandbox");
        stopped_tx.send(()).expect("send stopped signal");
        release_rx.recv().expect("release signal");
        stopped.delete().expect("delete first sandbox");
    });

    barrier.wait();
    let second = backend
        .admit(config(&vm_id))
        .expect("admit second sandbox")
        .launch();
    assert!(
        second.is_err(),
        "same vm_id launch must fail while the first run-dir is still owned"
    );

    stopped_rx.recv().expect("stopped signal");
    release_tx.send(()).expect("release stopper");
    stopper.join().expect("stopper thread");
}

fn config(vm_id: &str) -> SandboxConfig {
    SandboxConfig {
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
        request_id: None,
        pmem_layers: Vec::new(),
        preallocated_drive_slots: 0,
        one_shot: false,
    }
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
