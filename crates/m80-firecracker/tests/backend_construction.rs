//! Backend construction and reuse behavior.

mod common;

use std::sync::Arc;

use m80_firecracker::{Backend, BackendConfig, CgroupMode, FcError, NetworkPolicy, SandboxConfig};

fn sandbox_config() -> SandboxConfig {
    SandboxConfig {
        vm_id: None,
        workspace: None,
        network: NetworkPolicy::NoEgress,
        vcpu_count: None,
        mem_size_mib: None,
        boot_args: None,
        overlay_size_bytes: 512 * 1024 * 1024,
        idle_timeout: None,
        request_id: None,
    }
}

#[test]
fn backend_reused_across_admissions_shares_admission_state() {
    let dir = tempfile::tempdir().unwrap();
    let discovery = common::fake_discovery(dir.path());
    let backend = Arc::new(
        Backend::new(BackendConfig {
            discovery,
            max_concurrent_vms: 1,
            run_root: dir.path().to_path_buf(),
            jail_uid: 3000,
            jail_gid: 3000,
            cgroup_mode: CgroupMode::Disabled,
        })
        .expect("Backend::new"),
    );

    assert_eq!(backend.config().run_root, dir.path());
    assert_eq!(backend.config().discovery.run_root, dir.path());

    let first = backend
        .admit(sandbox_config())
        .expect("first admit acquires the singleton backend permit");

    let err = backend
        .admit(sandbox_config())
        .expect_err("same backend must share admission state across requests");
    assert!(
        matches!(err, FcError::AdmissionRefused { limit: 1 }),
        "expected shared admission refusal, got {err:?}"
    );

    drop(first);

    let second = backend
        .admit(sandbox_config())
        .expect("dropping the first sandbox returns the shared permit");
    drop(second);
}
