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

mod common;

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn end_to_end_real_kvm_boot_exec_stop_delete() {
    // Full preflight discovers the binaries and validates the environment.
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let backend = common::make_backend(discovery);
    let sandbox_config = common::sandbox_config_with_id("e2e-test");

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
    let backend = common::make_backend(discovery);
    let sandbox_config = m80_firecracker::SandboxConfig {
        daemonize: true,
        ..common::sandbox_config_with_id("e2e-daemonized-test")
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
    let backend = common::make_backend(discovery);
    let vm_id = format!("e2e-file-{:04x}", unique_suffix() % 0x10000);
    let sandbox_config = common::sandbox_config_with_id(vm_id);

    let sandbox = backend.admit(sandbox_config).expect("admit");
    let mut running = sandbox.launch().expect("launch");
    let run_dir = running.run_dir().to_path_buf();

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

    let work_dir = "/tmp/m80-fileops-work";
    assert!(running
        .create_dir(work_dir, Some(0o777), false)
        .expect("create exec-visible fileops work dir"));
    let blob_path = format!("{work_dir}/big.bin");
    let roundtrip_path = format!("{work_dir}/roundtrip.bin");

    let blob = deterministic_payload((5 * 1024 * 1024) + 123);
    let expected_hash = sha256_hex_bytes(&blob);
    let uploaded = running
        .upload_file_chunked(
            blob_path.as_str(),
            Some(0o644),
            std::io::Cursor::new(blob.clone()),
            1024 * 1024,
        )
        .expect("upload_file_chunked");
    assert_eq!(uploaded, blob.len() as u64);
    let stat = running
        .stat_file(blob_path.as_str())
        .expect("stat uploaded blob");
    assert_eq!(stat.size, blob.len() as u64);
    let copy = running
        .exec(m80_proto::ExecRequest {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), format!("cp {blob_path} {roundtrip_path}")],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(10_000),
            streaming: false,
        })
        .expect("copy uploaded blob inside guest");
    assert_eq!(copy.status, m80_proto::ExecStatus::Completed);
    assert_eq!(
        copy.exit_code,
        Some(0),
        "copy stderr={}",
        String::from_utf8_lossy(&copy.stderr)
    );
    let (read_blob, truncated) = running
        .read_file(roundtrip_path.as_str(), Some(blob.len() as u64))
        .expect("read uploaded blob");
    assert_eq!(read_blob, blob);
    assert_eq!(sha256_hex_bytes(&read_blob), expected_hash);
    assert!(!truncated);
    assert_run_dir_has_no_protocol_warnings(&run_dir);

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
    let host_mount_ns_before =
        std::fs::read_link("/proc/self/ns/mnt").expect("host mount ns before launch");
    let backend = common::make_backend(discovery);
    let sandbox_config = common::sandbox_config_with_id("e2e-jailer-security");

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
        host_mount_ns_before
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
    let host_path_inside_jail = format!("/proc/{pid}/root{}", env!("CARGO_MANIFEST_DIR"));
    assert!(
        std::fs::metadata(&host_path_inside_jail).is_err(),
        "host project path must not be visible through jailed root: {host_path_inside_jail}"
    );

    for (path, major, minor) in [
        ("dev/kvm", 10, 232),
        ("dev/net/tun", 10, 200),
        ("dev/urandom", 1, 9),
    ] {
        let meta = std::fs::metadata(format!("/proc/{pid}/root/{path}"))
            .unwrap_or_else(|e| panic!("{path} must exist in jail root: {e}"));
        assert!(
            meta.file_type().is_char_device(),
            "{path} must be a character device"
        );
        assert_eq!(device_major_minor(meta.rdev()), (major, minor));
    }
    assert_exec_file_is_private_copy(pid, &firecracker_bin, 3000, 3000);

    let stopped = running.stop().expect("stop");
    assert_eq!(
        std::fs::read_link("/proc/self/ns/mnt").expect("host mount ns after stop"),
        host_mount_ns_before,
        "jail teardown must leave the host test process in its original mount namespace"
    );
    assert_no_mountinfo_references(&run_dir);
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires root, iproute2 netns support, KVM host, and real Firecracker binary"]
fn end_to_end_real_kvm_join_netns_places_firecracker_in_requested_namespace() {
    let netns = NetnsGuard::create();
    netns.create_tap("tapm80struct");
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let backend = common::make_backend(discovery);
    let sandbox_config = m80_firecracker::SandboxConfig {
        network: join_netns_policy(&netns.path, "tapm80struct"),
        ..common::sandbox_config_with_id("e2e-join-netns")
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

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before Unix epoch")
        .as_nanos()
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

    fn create_tap(&self, tap_name: &str) {
        let status = Command::new("ip")
            .args(["netns", "exec", &self.name, "ip", "tuntap", "add", "dev"])
            .arg(tap_name)
            .args(["mode", "tap"])
            .status()
            .expect("run ip tuntap add");
        assert!(
            status.success(),
            "ip tuntap add {tap_name} in {} failed: {status}",
            self.name
        );
        let status = Command::new("ip")
            .args(["netns", "exec", &self.name, "ip", "link", "set"])
            .arg(tap_name)
            .arg("up")
            .status()
            .expect("run ip link set tap up");
        assert!(
            status.success(),
            "ip link set {tap_name} up in {} failed: {status}",
            self.name
        );
    }
}

impl Drop for NetnsGuard {
    fn drop(&mut self) {
        let _ = Command::new("ip")
            .args(["netns", "del", &self.name])
            .status();
    }
}

fn join_netns_policy(
    netns_path: &std::path::Path,
    tap_name: &str,
) -> m80_firecracker::NetworkPolicy {
    m80_firecracker::NetworkPolicy::JoinNetns {
        spec: m80_firecracker::NetnsSpec {
            netns_path: netns_path.to_path_buf(),
            tap_name: tap_name.to_owned(),
            guest_mac: m80_firecracker::MacAddr::parse("02:00:00:00:80:01")
                .expect("valid guest MAC"),
            guest_ipv4: "10.80.0.2/24".parse().unwrap(),
            gateway_ipv4: std::net::Ipv4Addr::new(10, 80, 0, 1),
            dns_resolvers: vec![std::net::Ipv4Addr::new(10, 80, 0, 1)],
        },
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

fn assert_no_mountinfo_references(path: &std::path::Path) {
    let mountinfo = std::fs::read_to_string("/proc/self/mountinfo").expect("host mountinfo");
    let needle = path.to_string_lossy();
    assert!(
        !mountinfo.contains(needle.as_ref()),
        "host mountinfo still references {} after stop:\n{mountinfo}",
        path.display()
    );
}

fn device_major_minor(rdev: u64) -> (u64, u64) {
    let major = ((rdev >> 8) & 0xfff) | ((rdev >> 32) & !0xfff);
    let minor = (rdev & 0xff) | ((rdev >> 12) & !0xff);
    (major, minor)
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

/// Fail if `<run_dir>/diagnostics.jsonl` contains an entry produced by
/// [`m80_firecracker::diagnostics::record_protocol_error`].
///
/// We match on `phase == "Request"` plus the canonical message prefix
/// `"protocol error stream_id="` rather than on raw substrings of forbidden
/// error class names. Substring matches over the full `diagnostics.jsonl` +
/// `console.log` text would false-positive on benign log lines that happen to
/// mention `ProtoError` Display fragments (e.g., `"malformed payload: ..."`)
/// without those lines actually being host-emitted protocol-error records.
/// `console.log` is the guest serial firehose and never carries host-side
/// protocol events; it is excluded.
fn assert_run_dir_has_no_protocol_warnings(run_dir: &std::path::Path) {
    let diag_path = run_dir.join("diagnostics.jsonl");
    let Ok(text) = std::fs::read_to_string(&diag_path) else {
        return;
    };
    for (idx, line) in text.lines().enumerate() {
        let entry: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let phase = entry.get("phase").and_then(|v| v.as_str()).unwrap_or("");
        let message = entry.get("message").and_then(|v| v.as_str()).unwrap_or("");
        if phase == "Request" && message.starts_with("protocol error stream_id=") {
            panic!(
                "unexpected protocol error in {} line {}: {entry}",
                diag_path.display(),
                idx + 1
            );
        }
    }
}
