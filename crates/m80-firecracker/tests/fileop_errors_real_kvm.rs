//! Real-KVM negative coverage for guest file-operation error surfaces.

mod common;

use std::time::{SystemTime, UNIX_EPOCH};

use m80_firecracker::{Backend, BackendConfig, CgroupMode, FcError, NetworkPolicy, SandboxConfig};
use m80_proto::FileError;

use common::RunDirDumpGuard;

fn launch_vm() -> (m80_firecracker::RunningSandbox, std::path::PathBuf) {
    let discovery = m80_preflight::run().expect("preflight");
    let run_root = discovery.run_root.clone();
    let config = BackendConfig {
        discovery,
        max_concurrent_vms: 1,
        run_root,
        jail_uid: 3000,
        jail_gid: 3000,
        cgroup_mode: CgroupMode::Disabled,
    };
    let backend = std::sync::Arc::new(Backend::new(config).expect("Backend::new"));
    let sandbox = backend
        .admit(SandboxConfig {
            vm_id: Some(unique_vm_id("fileop-err")),
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
    let running = sandbox.launch().expect("launch");
    let run_dir = running.run_dir().to_owned();
    (running, run_dir)
}

fn unique_vm_id(prefix: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis()
        % 1_000_000;
    format!("{prefix}-{millis}")
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn read_file_nonexistent_path_returns_not_found() {
    let (mut running, run_dir) = launch_vm();
    let _dump_guard = RunDirDumpGuard::new(run_dir);

    let err = running
        .read_file("/tmp/m80-definitely-missing-file", Some(1024))
        .expect_err("missing guest file must fail");

    assert!(matches!(err, FcError::FileOp(FileError::NotFound)));

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}
