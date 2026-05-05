//! Admission semaphore: permit counting, refusal, and permit return on drop.

mod common;

use std::sync::Arc;

use m80_firecracker::{Backend, BackendConfig, CgroupMode, FcError, NetworkPolicy, SandboxConfig};

fn make_backend(max: u32) -> Arc<Backend> {
    let config = BackendConfig {
        discovery: common::fake_discovery(std::path::Path::new("/tmp")),
        max_concurrent_vms: max,
        run_root: std::path::PathBuf::from("/tmp/m80-test"),
        jail_uid: 3000,
        jail_gid: 3000,
        cgroup_mode: CgroupMode::Disabled,
    };
    Arc::new(Backend::new(config).expect("Backend::new should not fail"))
}

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
    }
}

#[test]
fn admit_up_to_limit_succeeds() {
    let backend = make_backend(2);
    let s1 = backend
        .admit(sandbox_config())
        .expect("first admit should succeed");
    let s2 = backend
        .admit(sandbox_config())
        .expect("second admit should succeed");
    drop(s1);
    drop(s2);
}

#[test]
fn admit_beyond_limit_returns_refused() {
    let backend = make_backend(2);
    let s1 = backend
        .admit(sandbox_config())
        .expect("first admit should succeed");
    let s2 = backend
        .admit(sandbox_config())
        .expect("second admit should succeed");

    let err = backend
        .admit(sandbox_config())
        .expect_err("third admit should be refused");

    assert!(
        matches!(err, FcError::AdmissionRefused { limit: 2 }),
        "expected AdmissionRefused(limit=2), got {err:?}"
    );

    drop(s1);
    drop(s2);
}

#[test]
fn permit_drop_restores_slot() {
    let backend = make_backend(1);
    let s1 = backend
        .admit(sandbox_config())
        .expect("first admit should succeed");

    // At capacity — second admit fails.
    backend
        .admit(sandbox_config())
        .expect_err("should be refused at limit");

    // Dropping s1 returns the permit; next admit should succeed.
    drop(s1);

    let s2 = backend
        .admit(sandbox_config())
        .expect("admit after drop should succeed");
    drop(s2);
}
