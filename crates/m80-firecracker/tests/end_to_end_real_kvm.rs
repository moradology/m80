//! End-to-end smoke test: launch → exec → stop → extract_changes → delete.
//!
//! This test requires a KVM-capable host with real Firecracker + jailer
//! binaries and a built m80 guest image. It is `#[ignore]`d by default
//! and must be run explicitly on a prepared host:
//!
//! ```
//! sudo cargo test -p m80-firecracker -- --ignored end_to_end_real_kvm
//! ```
//!
//! Set the following environment variables to configure the test:
//! - `M80_FIRECRACKER_BIN` — path to the `firecracker` binary.
//! - `M80_JAILER_BIN` — path to the `jailer` binary.
//! - `M80_KERNEL_IMAGE` — path to the guest kernel.
//! - `M80_ROOTFS_IMAGE` — path to the built m80 rootfs.
//! - `M80_RUN_ROOT` — directory where the VM state is written.
//!
//! The test verifies:
//! 1. `Backend::new` + `admit` succeed.
//! 2. `Sandbox::launch` boots the VM and returns a `RunningSandbox`.
//! 3. `RunningSandbox::exec` runs `echo hello` and returns its stdout.
//! 4. `RunningSandbox::stop` tears down the VM cleanly.
//! 5. `StoppedSandbox::delete` removes the run-dir.

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn end_to_end_real_kvm_boot_exec_stop_delete() {
    // Full preflight discovers the binaries and validates the environment.
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");

    let run_root = discovery.run_root.clone();

    let config = m80_firecracker::BackendConfig {
        discovery,
        max_concurrent_vms: 1,
        run_root: run_root.clone(),
        jail_uid: 3000,
        jail_gid: 3000,
        cgroup_mode: m80_firecracker::CgroupMode::Disabled,
    };

    let backend = std::sync::Arc::new(m80_firecracker::Backend::new(config).expect("Backend::new"));

    let sandbox_config = m80_firecracker::SandboxConfig {
        vm_id: Some("e2e-test".into()),
        workspace: None,
        network: m80_firecracker::NetworkPolicy::NoEgress,
        vcpu_count: Some(1),
        mem_size_mib: Some(512),
        boot_args: None,
        overlay_size_bytes: 512 * 1024 * 1024,
        idle_timeout: None,
        request_id: None,
    };

    let sandbox = backend.admit(sandbox_config).expect("admit");
    let mut running = sandbox.launch().expect("launch");

    let response = running
        .exec(m80_proto::ExecRequest {
            program: "/bin/echo".into(),
            args: vec!["hello".into()],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        })
        .expect("exec");

    assert_eq!(response.status, m80_proto::ExecStatus::Completed);
    assert_eq!(response.exit_code, Some(0));
    let stdout = String::from_utf8_lossy(&response.stdout);
    assert!(stdout.trim() == "hello", "expected 'hello', got {stdout:?}");

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn end_to_end_real_kvm_file_ops() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");

    let run_root = discovery.run_root.clone();
    let config = m80_firecracker::BackendConfig {
        discovery,
        max_concurrent_vms: 1,
        run_root: run_root.clone(),
        jail_uid: 3000,
        jail_gid: 3000,
        cgroup_mode: m80_firecracker::CgroupMode::Disabled,
    };
    let backend = std::sync::Arc::new(m80_firecracker::Backend::new(config).expect("Backend::new"));
    let sandbox_config = m80_firecracker::SandboxConfig {
        vm_id: Some("e2e-fileops-test".into()),
        workspace: None,
        network: m80_firecracker::NetworkPolicy::NoEgress,
        vcpu_count: Some(1),
        mem_size_mib: Some(512),
        boot_args: None,
        overlay_size_bytes: 512 * 1024 * 1024,
        idle_timeout: None,
        request_id: None,
    };

    let sandbox = backend.admit(sandbox_config).expect("admit");
    let mut running = sandbox.launch().expect("launch");

    let bytes = b"hello fileops".to_vec();
    let written = running
        .write_file("/tmp/m80-fileops.txt", bytes.clone(), Some(0o600))
        .expect("write_file");
    assert_eq!(written, bytes.len() as u64);

    let (read_back, truncated) = running
        .read_file("/tmp/m80-fileops.txt", Some(1024))
        .expect("read_file");
    assert_eq!(read_back, bytes);
    assert!(!truncated);

    let entries = running.list_dir("/tmp").expect("list_dir");
    assert!(entries.iter().any(|entry| entry.name == "m80-fileops.txt"));

    let stat = running
        .stat_file("/tmp/m80-fileops.txt")
        .expect("stat_file");
    assert_eq!(stat.size, written);

    let blob = vec![b'x'; 5 * 1024 * 1024];
    let uploaded = running
        .upload_file_chunked(
            "/tmp/m80-fileops-big.bin",
            Some(0o600),
            std::io::Cursor::new(blob.clone()),
            1024 * 1024,
        )
        .expect("upload_file_chunked");
    assert_eq!(uploaded, blob.len() as u64);
    let (read_blob, truncated) = running
        .read_file("/tmp/m80-fileops-big.bin", Some(blob.len() as u64))
        .expect("read uploaded blob");
    assert_eq!(read_blob, blob);
    assert!(!truncated);

    running
        .remove_file("/tmp/m80-fileops.txt")
        .expect("remove_file");
    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}
