//! Overlay-rootfs lifecycle test.
//!
//! Ignored by default because it requires a KVM-capable host with real
//! Firecracker + jailer binaries, a built m80 guest image, and `debugfs`.

mod common;

use std::process::Command;

use common::RunDirDumpGuard;

use m80_firecracker::{Backend, BackendConfig, CgroupMode, NetworkPolicy, SandboxConfig};
use m80_proto::{ExecRequest, ExecStatus};

fn debugfs_cat(image: &std::path::Path, path: &str) -> String {
    let output = Command::new("debugfs")
        .arg("-R")
        .arg(format!("cat {path}"))
        .arg(image)
        .output()
        .expect("debugfs must run");
    assert!(
        output.status.success(),
        "debugfs cat {path} failed: status={:?}\nstdout={}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn overlay_allocated_bytes(path: &std::path::Path) -> u64 {
    use std::os::unix::fs::MetadataExt as _;

    std::fs::metadata(path)
        .unwrap_or_else(|e| panic!("metadata {}: {e}", path.display()))
        .blocks()
        * 512
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary and debugfs"]
fn overlay_pivot_writes_land_in_overlay_and_base_stays_verified() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let manifest_dir = discovery
        .rootfs
        .parent()
        .unwrap_or_else(|| std::path::Path::new("/"));
    discovery
        .manifest
        .verify(manifest_dir)
        .expect("base artifacts must verify before launch");

    let run_root = discovery.run_root.clone();
    let backend = std::sync::Arc::new(
        Backend::new(
            BackendConfig::builder(discovery.clone())
                .max_concurrent_vms(1)
                .run_root(run_root)
                .jail_uid(3000)
                .jail_gid(3000)
                .cgroup_mode(CgroupMode::Disabled)
                .build(),
        )
        .expect("Backend::new"),
    );

    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock must be after UNIX_EPOCH")
        .as_nanos()
        % 1_000_000_000;
    let vm_id = format!("ovl-{}-{unique}", std::process::id());
    let sandbox = backend
        .admit(SandboxConfig {
            vm_id: Some(vm_id),
            workspace: None,
            network: NetworkPolicy::NoEgress,
            vcpu_count: Some(1),
            mem_size_mib: Some(512),
            cpuset_cpus: None,
            cpu_template: None,
            fc_log_level: None,
            drive_cache_type: None,
            boot_args: None,
            overlay_size_bytes: 512 * 1024 * 1024,
            overlay_clone_mode: Default::default(),
            idle_timeout: None,
            daemonize: false,
            request_id: None,
            pmem_layers: Vec::new(),
            preallocated_drive_slots: 0,
            one_shot: false,
        })
        .expect("admit");
    let mut running = sandbox.launch().expect("launch");
    let _dump = RunDirDumpGuard::new(running.run_dir().to_path_buf());

    let marker = "m80-overlay-probe";
    let response = running
        .exec(ExecRequest {
            program: "/bin/sh".to_string(),
            args: vec![
                "-c".to_string(),
                format!("printf '{marker}' > /m80-probe && mount"),
            ],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        })
        .expect("exec overlay probe");
    assert_eq!(response.status, ExecStatus::Completed);
    assert_eq!(response.exit_code, Some(0));
    let mount_output = String::from_utf8_lossy(&response.stdout);
    assert!(
        mount_output.contains("overlay on / type overlay"),
        "mount output must show overlayfs at /:\n{mount_output}"
    );
    assert!(
        !mount_output.contains(" on /lower "),
        "old lower mount must not remain visible after pivot:\n{mount_output}"
    );

    let stopped = running.stop().expect("stop");
    let overlay = stopped.run_dir().join("rootfs.overlay.ext4");
    discovery
        .manifest
        .verify(manifest_dir)
        .expect("base artifacts must still verify after launch");
    let upper_probe = debugfs_cat(&overlay, "/root/m80-probe");
    assert_eq!(upper_probe, marker);

    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary and debugfs"]
fn overlay_immutability_lower_unchanged_after_upper_write() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let manifest_dir = discovery
        .rootfs
        .parent()
        .unwrap_or_else(|| std::path::Path::new("/"));
    discovery
        .manifest
        .verify(manifest_dir)
        .expect("base artifacts must verify before launch");
    let run_root = discovery.run_root.clone();
    let backend = std::sync::Arc::new(
        Backend::new(
            BackendConfig::builder(discovery.clone())
                .max_concurrent_vms(1)
                .run_root(run_root)
                .jail_uid(3000)
                .jail_gid(3000)
                .cgroup_mode(CgroupMode::Disabled)
                .build(),
        )
        .expect("Backend::new"),
    );

    let vm_id = format!("ovlimm-{}", std::process::id());
    let sandbox = backend
        .admit(SandboxConfig {
            vm_id: Some(vm_id),
            workspace: None,
            network: NetworkPolicy::NoEgress,
            vcpu_count: Some(1),
            mem_size_mib: Some(512),
            cpuset_cpus: None,
            cpu_template: None,
            fc_log_level: None,
            drive_cache_type: None,
            boot_args: None,
            overlay_size_bytes: 512 * 1024 * 1024,
            overlay_clone_mode: Default::default(),
            idle_timeout: None,
            daemonize: false,
            request_id: Some("req-overlay-immutability".into()),
            pmem_layers: Vec::new(),
            preallocated_drive_slots: 0,
            one_shot: false,
        })
        .expect("admit");
    let mut running = sandbox.launch().expect("launch");
    let _dump = RunDirDumpGuard::new(running.run_dir().to_path_buf());

    let target = "/bin/busybox";
    let marker = "m80-overlay-copy-up\n";
    let response = running
        .exec(ExecRequest {
            program: "/bin/sh".to_string(),
            args: vec!["-c".to_string(), format!("printf '{marker}' > {target}")],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        })
        .expect("exec lower write");
    assert_eq!(response.status, ExecStatus::Completed);
    assert_eq!(
        response.exit_code,
        Some(0),
        "lower write failed for {target}: stdout={} stderr={}",
        String::from_utf8_lossy(&response.stdout),
        String::from_utf8_lossy(&response.stderr)
    );

    let stopped = running.stop().expect("stop");
    discovery
        .manifest
        .verify(manifest_dir)
        .expect("base artifacts must still verify after lower-layer write");
    let overlay = stopped.run_dir().join("rootfs.overlay.ext4");
    let upper_path = format!("/root{target}");
    let upper_file = debugfs_cat(&overlay, &upper_path);
    assert_eq!(upper_file, marker);

    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn overlay_grows_under_sustained_guest_writes() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let run_root = discovery.run_root.clone();
    let backend = std::sync::Arc::new(
        Backend::new(
            BackendConfig::builder(discovery)
                .max_concurrent_vms(1)
                .run_root(run_root)
                .jail_uid(3000)
                .jail_gid(3000)
                .cgroup_mode(CgroupMode::Disabled)
                .build(),
        )
        .expect("Backend::new"),
    );

    let vm_id = format!("overlay-growth-{}", std::process::id());
    let sandbox = backend
        .admit(SandboxConfig {
            vm_id: Some(vm_id),
            workspace: None,
            network: NetworkPolicy::NoEgress,
            vcpu_count: Some(1),
            mem_size_mib: Some(512),
            cpuset_cpus: None,
            cpu_template: None,
            fc_log_level: None,
            drive_cache_type: None,
            boot_args: None,
            overlay_size_bytes: 512 * 1024 * 1024,
            overlay_clone_mode: Default::default(),
            idle_timeout: None,
            daemonize: false,
            request_id: Some("req-overlay-growth".into()),
            pmem_layers: Vec::new(),
            preallocated_drive_slots: 0,
            one_shot: false,
        })
        .expect("admit");
    let mut running = sandbox.launch().expect("launch");
    let run_dir = running.run_dir().to_path_buf();
    let _dump = RunDirDumpGuard::new(run_dir.clone());
    let overlay = run_dir.join("rootfs.overlay.ext4");

    let before = overlay_allocated_bytes(&overlay);
    write_guest_file(&mut running, "/tmp/m80-overlay-growth-1.bin", 5);
    let after_first = overlay_allocated_bytes(&overlay);
    write_guest_file(&mut running, "/tmp/m80-overlay-growth-2.bin", 5);
    let after_second = overlay_allocated_bytes(&overlay);

    assert!(
        after_first > before,
        "first guest write must allocate host overlay blocks: before={before}, after_first={after_first}"
    );
    assert!(
        after_second > after_first,
        "second guest write must allocate more host overlay blocks: after_first={after_first}, after_second={after_second}"
    );

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn three_drive_order_mounts_workspace_as_vdc_and_preserves_base() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let manifest_dir = discovery
        .rootfs
        .parent()
        .unwrap_or_else(|| std::path::Path::new("/"));
    discovery
        .manifest
        .verify(manifest_dir)
        .expect("base artifacts must verify before launch");
    let run_root = discovery.run_root.clone();
    let backend = std::sync::Arc::new(
        Backend::new(
            BackendConfig::builder(discovery.clone())
                .max_concurrent_vms(1)
                .run_root(run_root.clone())
                .jail_uid(3000)
                .jail_gid(3000)
                .cgroup_mode(CgroupMode::Disabled)
                .build(),
        )
        .expect("Backend::new"),
    );

    let vm_id = format!("drive-order-{}", std::process::id());
    let host_workspace = tempfile::tempdir().expect("workspace tempdir");
    let sandbox = backend
        .admit(SandboxConfig {
            vm_id: Some(vm_id.clone()),
            workspace: Some(host_workspace.path().to_path_buf()),
            network: NetworkPolicy::NoEgress,
            vcpu_count: Some(1),
            mem_size_mib: Some(512),
            cpuset_cpus: None,
            cpu_template: None,
            fc_log_level: None,
            drive_cache_type: None,
            boot_args: None,
            overlay_size_bytes: 512 * 1024 * 1024,
            overlay_clone_mode: Default::default(),
            idle_timeout: None,
            daemonize: false,
            request_id: Some("req-three-drive-order".into()),
            pmem_layers: Vec::new(),
            preallocated_drive_slots: 0,
            one_shot: false,
        })
        .expect("admit");
    let mut running = sandbox.launch().expect("launch");
    let _dump = RunDirDumpGuard::new(run_root.join(&vm_id));

    let response = running
        .exec(ExecRequest {
            program: "/bin/sh".to_string(),
            args: vec![
                "-c".to_string(),
                "set -eu; \
                 cat /proc/mounts; \
                 printf 'cmdline='; cat /proc/cmdline; printf '\\n'; \
                 printf ok > /workspace/drive-order.txt; \
                 cat /workspace/drive-order.txt"
                    .to_string(),
            ],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(10_000),
            streaming: false,
        })
        .expect("exec drive-order probe");
    assert_eq!(response.status, ExecStatus::Completed);
    assert_eq!(
        response.exit_code,
        Some(0),
        "drive-order probe failed: stdout={} stderr={}",
        String::from_utf8_lossy(&response.stdout),
        String::from_utf8_lossy(&response.stderr)
    );
    let stdout = String::from_utf8_lossy(&response.stdout);
    assert!(
        stdout.contains("overlay / overlay "),
        "root must be the overlayfs merged view:\n{stdout}"
    );
    assert!(
        stdout.contains("/dev/vdc /workspace ext4 "),
        "workspace must be the third Firecracker drive, not the overlay upper:\n{stdout}"
    );
    assert!(
        stdout.contains("m80.workspace=1"),
        "boot args must mark the workspace drive as present:\n{stdout}"
    );
    assert!(
        stdout.trim_end().ends_with("ok"),
        "workspace write/read probe failed:\n{stdout}"
    );

    let stopped = running.stop().expect("stop");
    discovery
        .manifest
        .verify(manifest_dir)
        .expect("base artifacts must still verify after workspace write");
    stopped.delete().expect("delete");
}

fn write_guest_file(running: &mut m80_firecracker::RunningSandbox, path: &str, mib: u32) {
    let response = running
        .exec(ExecRequest {
            program: "/bin/sh".to_string(),
            args: vec![
                "-c".to_string(),
                format!("dd if=/dev/zero of={path} bs=1M count={mib} conv=fsync && sync"),
            ],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(10_000),
            streaming: false,
        })
        .expect("write guest file");
    assert_eq!(response.status, ExecStatus::Completed);
    assert_eq!(
        response.exit_code,
        Some(0),
        "guest write failed: stdout={} stderr={}",
        String::from_utf8_lossy(&response.stdout),
        String::from_utf8_lossy(&response.stderr)
    );
}
