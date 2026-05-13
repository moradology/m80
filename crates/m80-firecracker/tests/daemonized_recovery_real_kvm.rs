//! Real-KVM proof for daemonized jailer recovery.
//!
//! The official jailer exits after daemonizing Firecracker, so m80 records
//! `jailer_pid = 0` as a sentinel. Recovery must treat that sentinel as live
//! when the recorded Firecracker PID is still running.

mod common;

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn jailer_daemonized_pid_sentinel_recovery() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let run_root = discovery.run_root.clone();

    let config = m80_firecracker::BackendConfig {
        discovery,
        max_concurrent_vms: 1,
        run_root,
        jail_uid: 3000,
        jail_gid: 3000,
        cgroup_mode: m80_firecracker::CgroupMode::Disabled,
    };
    let backend = std::sync::Arc::new(m80_firecracker::Backend::new(config).expect("Backend::new"));
    let sandbox_config = m80_firecracker::SandboxConfig {
        vm_id: Some("e2e-daemonized-recovery".into()),
        workspace: None,
        network: m80_firecracker::NetworkPolicy::NoEgress,
        vcpu_count: Some(1),
        mem_size_mib: Some(512),
        cpu_template: None,
        boot_args: None,
        overlay_size_bytes: 512 * 1024 * 1024,
        idle_timeout: None,
        daemonize: true,
        request_id: Some("req-daemonized-recovery".into()),
        preallocated_drive_slots: 0,
        one_shot: false,
    };

    let sandbox = backend.admit(sandbox_config).expect("admit");
    let running = sandbox.launch().expect("launch");
    let run_dir = running.run_dir().to_path_buf();
    let _dump_guard = common::RunDirDumpGuard::new(run_dir.clone());

    let state: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(run_dir.join("jailer-state.json")).unwrap())
            .unwrap();
    assert_eq!(state["jailer_pid"], 0);
    let firecracker_pid = state["firecracker_pid"].as_u64().expect("firecracker_pid") as u32;
    assert_proc_pid_live(firecracker_pid);

    let decision = m80_jailer::inspect_run_dir(&run_dir).expect("inspect run-dir");
    match decision {
        m80_jailer::InspectionDecision::LiveJail {
            jailer_pid,
            firecracker_pid: recovered_firecracker_pid,
        } => {
            assert_eq!(jailer_pid, 0);
            assert_eq!(recovered_firecracker_pid, firecracker_pid);
        }
        other => panic!("expected LiveJail for daemonized sentinel, got {other:?}"),
    }
    assert_proc_pid_live(firecracker_pid);

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

fn assert_proc_pid_live(pid: u32) {
    assert!(
        std::path::PathBuf::from(format!("/proc/{pid}")).exists(),
        "pid {pid} must be live"
    );
}
