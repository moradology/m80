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

use std::io::Write as _;
use std::process::{Command, Stdio};

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

    let blob = deterministic_payload((5 * 1024 * 1024) + 123);
    let expected_hash = sha256_hex_bytes(&blob);
    let uploaded = running
        .upload_file_chunked(
            "/tmp/m80-fileops-big.bin",
            Some(0o600),
            std::io::Cursor::new(blob.clone()),
            1024 * 1024,
        )
        .expect("upload_file_chunked");
    assert_eq!(uploaded, blob.len() as u64);
    let stat = running
        .stat_file("/tmp/m80-fileops-big.bin")
        .expect("stat uploaded blob");
    assert_eq!(stat.size, blob.len() as u64);
    let guest_hash = running
        .exec(m80_proto::ExecRequest {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                "bytes=$(wc -c < /tmp/m80-fileops-big.bin); \
                 hash=$(sha256sum /tmp/m80-fileops-big.bin | cut -d ' ' -f1); \
                 printf '%s %s\\n' \"$bytes\" \"$hash\""
                    .into(),
            ],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(10_000),
            streaming: false,
        })
        .expect("guest hash uploaded blob");
    assert_eq!(guest_hash.status, m80_proto::ExecStatus::Completed);
    assert_eq!(guest_hash.exit_code, Some(0));
    let guest_hash_stdout = String::from_utf8_lossy(&guest_hash.stdout);
    assert_eq!(
        guest_hash_stdout.trim(),
        format!("{} {expected_hash}", blob.len())
    );

    running
        .exec(m80_proto::ExecRequest {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                "cp /tmp/m80-fileops-big.bin /tmp/m80-fileops-roundtrip.bin".into(),
            ],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(10_000),
            streaming: false,
        })
        .expect("copy uploaded blob inside guest");
    let (read_blob, truncated) = running
        .read_file("/tmp/m80-fileops-roundtrip.bin", Some(blob.len() as u64))
        .expect("read uploaded blob");
    assert_eq!(read_blob, blob);
    assert_eq!(sha256_hex_bytes(&read_blob), expected_hash);
    assert!(!truncated);
    assert_no_protocol_warnings(&run_root);

    running
        .remove_file("/tmp/m80-fileops.txt")
        .expect("remove_file");
    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

fn deterministic_payload(len: usize) -> Vec<u8> {
    let mut state = 0x4d80_cafe_u64;
    let mut out = Vec::with_capacity(len);
    for _ in 0..len {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        out.push((state >> 32) as u8);
    }
    out
}

fn sha256_hex_bytes(bytes: &[u8]) -> String {
    let mut child = Command::new("sha256sum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn sha256sum");
    child
        .stdin
        .as_mut()
        .expect("sha256sum stdin")
        .write_all(bytes)
        .expect("write sha256sum stdin");
    let output = child.wait_with_output().expect("wait sha256sum");
    assert!(output.status.success(), "sha256sum failed: {output:?}");
    String::from_utf8(output.stdout)
        .expect("sha256sum utf8")
        .split_whitespace()
        .next()
        .expect("sha256 hex")
        .to_owned()
}

fn assert_no_protocol_warnings(run_root: &std::path::Path) {
    let mut text = String::new();
    for entry in std::fs::read_dir(run_root).expect("read run root") {
        let path = entry.expect("run root entry").path();
        for name in ["console.log", "diagnostics.jsonl"] {
            let file = path.join(name);
            if let Ok(contents) = std::fs::read_to_string(file) {
                text.push_str(&contents);
            }
        }
    }
    for needle in [
        "OversizedPayload",
        "oversized payload",
        "malformed frame",
        "malformed payload",
        "unexpected EOF",
        "disconnect before terminal",
    ] {
        assert!(
            !text.contains(needle),
            "unexpected protocol warning {needle:?} in run logs:\n{text}"
        );
    }
}
