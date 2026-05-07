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
        Backend::new(BackendConfig {
            discovery: discovery.clone(),
            max_concurrent_vms: 1,
            run_root: run_root.clone(),
            jail_uid: 3000,
            jail_gid: 3000,
            cgroup_mode: CgroupMode::Disabled,
        })
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
            boot_args: None,
            overlay_size_bytes: 512 * 1024 * 1024,
            idle_timeout: None,
            daemonize: false,
            request_id: None,
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
