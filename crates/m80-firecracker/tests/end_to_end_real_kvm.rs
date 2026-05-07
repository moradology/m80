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
//! - `M80_JAILER_HARDEN_BIN` — path to `m80-jailer-harden`.
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
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

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
        daemonize: false,
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
fn end_to_end_real_kvm_daemonized_boot_exec_stop_delete() {
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
        vm_id: Some("e2e-daemonized-test".into()),
        workspace: None,
        network: m80_firecracker::NetworkPolicy::NoEgress,
        vcpu_count: Some(1),
        mem_size_mib: Some(512),
        boot_args: None,
        overlay_size_bytes: 512 * 1024 * 1024,
        idle_timeout: None,
        daemonize: true,
        request_id: None,
    };

    let sandbox = backend.admit(sandbox_config).expect("admit");
    let mut running = sandbox.launch().expect("launch");
    let run_dir = running.run_dir().to_path_buf();
    let state: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(run_dir.join("jailer-state.json")).unwrap())
            .unwrap();
    assert_eq!(state["jailer_pid"], 0);
    let pid = state["firecracker_pid"].as_u64().expect("firecracker_pid") as u32;
    assert!(
        std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "daemonized firecracker pid must be live"
    );

    let response = running
        .exec(m80_proto::ExecRequest {
            program: "/bin/echo".into(),
            args: vec!["daemonized".into()],
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
    assert!(
        stdout.trim() == "daemonized",
        "expected 'daemonized', got {stdout:?}"
    );

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
        daemonize: false,
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

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn end_to_end_real_kvm_jailer_security_parity() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let firecracker_bin = discovery.firecracker_bin.clone();

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
        vm_id: Some("e2e-jailer-security".into()),
        workspace: None,
        network: m80_firecracker::NetworkPolicy::NoEgress,
        vcpu_count: Some(1),
        mem_size_mib: Some(512),
        boot_args: None,
        overlay_size_bytes: 512 * 1024 * 1024,
        idle_timeout: None,
        daemonize: false,
        request_id: None,
    };

    let sandbox = backend.admit(sandbox_config).expect("admit");
    let running = sandbox.launch().expect("launch");
    let run_dir = running.run_dir().to_path_buf();
    let state: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(run_dir.join("jailer-state.json")).unwrap())
            .unwrap();
    let pid = state["firecracker_pid"].as_u64().expect("firecracker_pid") as u32;

    assert_limit_contains(pid, "Max open files", "2048", "2048");
    assert_status_line(pid, "Uid:", "3000\t3000\t3000\t3000");
    assert_status_line(pid, "Gid:", "3000\t3000\t3000\t3000");
    assert_status_line(pid, "NoNewPrivs:", "1");
    assert_status_line(pid, "CapPrm:", "0000000000000000");
    assert_status_line(pid, "CapEff:", "0000000000000000");
    assert_status_line(pid, "CapInh:", "0000000000000000");
    assert_status_line(pid, "CapAmb:", "0000000000000000");
    assert_status_line(pid, "SigBlk:", "0000000000000000");
    assert_supplementary_groups_empty(pid);
    assert_ne!(
        std::fs::read_link(format!("/proc/{pid}/ns/mnt")).expect("firecracker mount ns"),
        std::fs::read_link("/proc/self/ns/mnt").expect("host mount ns")
    );
    std::fs::metadata(format!("/proc/{pid}/root/kernel"))
        .expect("firecracker root must expose the jailed kernel binding");

    for mount_point in ["/kernel", "/rootfs.ext4", "/rootfs.overlay.ext4"] {
        let options = mount_options(pid, mount_point);
        assert!(
            options.contains("nosuid") && options.contains("nodev") && options.contains("noexec"),
            "{mount_point} options missing hardening flags: {options}"
        );
    }
    let rootfs_options = mount_options(pid, "/rootfs.ext4");
    assert!(
        rootfs_options.contains("ro"),
        "rootfs bind must be read-only: {rootfs_options}"
    );

    for path in ["dev/kvm", "dev/net/tun", "dev/urandom"] {
        let meta = std::fs::metadata(format!("/proc/{pid}/root/{path}"))
            .unwrap_or_else(|e| panic!("{path} must exist in jail root: {e}"));
        assert!(
            meta.file_type().is_char_device(),
            "{path} must be a character device"
        );
    }
    assert_exec_file_is_private_copy(pid, &firecracker_bin, 3000, 3000);

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires root, iproute2 netns support, KVM host, and real Firecracker binary"]
fn end_to_end_real_kvm_join_netns_places_firecracker_in_requested_namespace() {
    let netns = NetnsGuard::create();
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
        vm_id: Some("e2e-join-netns".into()),
        workspace: None,
        network: m80_firecracker::NetworkPolicy::JoinNetns {
            netns_path: netns.path.clone(),
        },
        vcpu_count: Some(1),
        mem_size_mib: Some(512),
        boot_args: None,
        overlay_size_bytes: 512 * 1024 * 1024,
        idle_timeout: None,
        daemonize: false,
        request_id: None,
    };

    let sandbox = backend.admit(sandbox_config).expect("admit");
    let running = sandbox.launch().expect("launch");
    let run_dir = running.run_dir().to_path_buf();
    let state: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(run_dir.join("jailer-state.json")).unwrap())
            .unwrap();
    let pid = state["firecracker_pid"].as_u64().expect("firecracker_pid") as u32;

    assert_same_network_namespace(pid, &netns.path);

    let stopped = running.stop().expect("stop");
    assert_run_dir_has_no_protocol_warnings(&run_dir);
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

fn assert_limit_contains(pid: u32, label: &str, soft: &str, hard: &str) {
    let limits = std::fs::read_to_string(format!("/proc/{pid}/limits")).expect("limits");
    let line = limits
        .lines()
        .find(|line| line.starts_with(label))
        .unwrap_or_else(|| panic!("missing {label} in limits:\n{limits}"));
    assert!(
        line.contains(soft) && line.contains(hard),
        "{label} line does not contain expected soft/hard limits {soft}/{hard}: {line}"
    );
}

fn assert_status_line(pid: u32, label: &str, expected: &str) {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).expect("status");
    let line = status
        .lines()
        .find(|line| line.starts_with(label))
        .unwrap_or_else(|| panic!("missing {label} in status:\n{status}"));
    assert!(
        line.contains(expected),
        "{label} line does not contain {expected:?}: {line}"
    );
}

fn assert_supplementary_groups_empty(pid: u32) {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).expect("status");
    let line = status
        .lines()
        .find(|line| line.starts_with("Groups:"))
        .unwrap_or_else(|| panic!("missing Groups in status:\n{status}"));
    assert_eq!(line.trim(), "Groups:", "supplementary groups must be empty");
}

struct NetnsGuard {
    path: std::path::PathBuf,
    name: String,
}

impl NetnsGuard {
    fn create() -> Self {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let name = format!("m80-e2e-{suffix}");
        let status = Command::new("ip")
            .args(["netns", "add", &name])
            .status()
            .expect("run ip netns add");
        assert!(status.success(), "ip netns add {name} failed: {status}");
        Self {
            path: std::path::PathBuf::from(format!("/var/run/netns/{name}")),
            name,
        }
    }
}

impl Drop for NetnsGuard {
    fn drop(&mut self) {
        let _ = Command::new("ip")
            .args(["netns", "del", &self.name])
            .status();
    }
}

fn assert_same_network_namespace(pid: u32, expected_netns: &std::path::Path) {
    let process_netns = format!("/proc/{pid}/ns/net");
    let expected = std::fs::metadata(expected_netns).expect("expected netns metadata");
    let actual = std::fs::metadata(&process_netns).expect("firecracker netns metadata");
    assert_eq!(
        (actual.dev(), actual.ino()),
        (expected.dev(), expected.ino()),
        "firecracker must run inside the requested network namespace"
    );
}

fn mount_options(pid: u32, mount_point: &str) -> String {
    let mountinfo = std::fs::read_to_string(format!("/proc/{pid}/mountinfo")).expect("mountinfo");
    for line in mountinfo.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.get(4) == Some(&mount_point) {
            return fields
                .get(5)
                .unwrap_or_else(|| panic!("missing options for {mount_point}: {line}"))
                .to_string();
        }
    }
    panic!("missing {mount_point} in mountinfo:\n{mountinfo}");
}

fn assert_exec_file_is_private_copy(pid: u32, source: &std::path::Path, uid: u32, gid: u32) {
    let copied = format!("/proc/{pid}/root/firecracker");
    let source_meta = std::fs::metadata(source).expect("source firecracker metadata");
    let copied_meta = std::fs::metadata(&copied).expect("copied firecracker metadata");

    assert_ne!(
        (source_meta.dev(), source_meta.ino()),
        (copied_meta.dev(), copied_meta.ino()),
        "jailed firecracker binary must be a copy, not a bind mount or hard link"
    );
    assert_eq!(
        copied_meta.nlink(),
        1,
        "copied binary must not be hard-linked"
    );
    assert_eq!(copied_meta.uid(), uid, "copied binary uid");
    assert_eq!(copied_meta.gid(), gid, "copied binary gid");
    assert_eq!(
        copied_meta.mode() & 0o777,
        0o700,
        "copied binary mode must be owner-only under m80 hardening"
    );
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
        append_protocol_logs(&mut text, &path);
    }
    assert_protocol_logs_are_clean(&text);
}

fn assert_run_dir_has_no_protocol_warnings(run_dir: &std::path::Path) {
    let mut text = String::new();
    append_protocol_logs(&mut text, run_dir);
    assert_protocol_logs_are_clean(&text);
}

fn append_protocol_logs(text: &mut String, run_dir: &std::path::Path) {
    for name in ["console.log", "diagnostics.jsonl"] {
        let file = run_dir.join(name);
        if let Ok(contents) = std::fs::read_to_string(file) {
            text.push_str(&contents);
        }
    }
}

fn assert_protocol_logs_are_clean(text: &str) {
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
