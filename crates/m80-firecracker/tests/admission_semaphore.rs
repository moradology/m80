//! Admission semaphore: permit counting, refusal, and permit return on drop.

use std::sync::Arc;

use m80_firecracker::{
    Backend, BackendConfig, CgroupMode, FcError, NetworkPolicy, SandboxConfig,
};

fn fake_manifest() -> m80_image_manifest::Manifest {
    m80_image_manifest::Manifest {
        schema_version: 1,
        expected_firecracker_version: "v1.0.0".into(),
        kernel_image: std::path::PathBuf::from("/dev/null"),
        kernel_image_sha256: "0".repeat(64),
        output_rootfs_image: std::path::PathBuf::from("/dev/null"),
        output_rootfs_sha256: "0".repeat(64),
        source_rootfs_image: std::path::PathBuf::from("/dev/null"),
        source_rootfs_sha256: "0".repeat(64),
        daemon_binary_path: std::path::PathBuf::from("/dev/null"),
        daemon_binary_sha256: "0".repeat(64),
        service_unit_path: std::path::PathBuf::from("/dev/null"),
        service_unit_sha256: "0".repeat(64),
        workspace_mount_path: std::path::PathBuf::from("/dev/null"),
        workspace_mount_sha256: "0".repeat(64),
        boot_target: "multi-user.target".into(),
        guest_port: 9001,
        no_egress_reason: None,
        ready_marker: "GUESTD_READY".into(),
    }
}

fn fake_discovery() -> m80_preflight::Discovery {
    m80_preflight::Discovery {
        firecracker_bin: std::path::PathBuf::from("/dev/null"),
        jailer_bin: std::path::PathBuf::from("/dev/null"),
        kernel: std::path::PathBuf::from("/dev/null"),
        rootfs: std::path::PathBuf::from("/dev/null"),
        manifest: fake_manifest(),
        run_root: std::path::PathBuf::from("/tmp"),
        privilege: m80_preflight::PrivilegeStatus::Root,
        report: vec![],
    }
}

fn make_backend(max: u32) -> Arc<Backend> {
    let config = BackendConfig {
        discovery: fake_discovery(),
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
        default_exec_timeout: None,
    }
}

#[test]
fn admit_up_to_limit_succeeds() {
    let backend = make_backend(2);
    let s1 = backend.admit(sandbox_config()).expect("first admit should succeed");
    let s2 = backend.admit(sandbox_config()).expect("second admit should succeed");
    drop(s1);
    drop(s2);
}

#[test]
fn admit_beyond_limit_returns_refused() {
    let backend = make_backend(2);
    let s1 = backend.admit(sandbox_config()).expect("first admit should succeed");
    let s2 = backend.admit(sandbox_config()).expect("second admit should succeed");

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
    let s1 = backend.admit(sandbox_config()).expect("first admit should succeed");

    // At capacity — second admit fails.
    backend
        .admit(sandbox_config())
        .expect_err("should be refused at limit");

    // Dropping s1 returns the permit; next admit should succeed.
    drop(s1);

    let s2 = backend.admit(sandbox_config()).expect("admit after drop should succeed");
    drop(s2);
}
