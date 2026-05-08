//! Real-KVM image-kind dispatch coverage for Minimal and Ubuntu artifacts.

mod common;

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use m80_firecracker::{Backend, BackendConfig, CgroupMode, NetworkPolicy, SandboxConfig};
use m80_image_manifest::ImageKind;
use m80_proto::{ExecRequest, ExecStatus};

use common::RunDirDumpGuard;

fn artifact_dir(env_key: &str, default_path: &str) -> PathBuf {
    std::env::var_os(env_key)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default_path))
}

fn discovery_for_artifacts(dir: &Path) -> m80_preflight::Discovery {
    let artifact_config = m80_preflight::ArtifactPreflightConfig {
        kernel_image: Some(dir.join("vmlinux")),
        artifact_dir: dir.to_owned(),
        rootfs_image: Some(dir.join("output.ext4")),
        kernel_kind: None,
        ..m80_preflight::ArtifactPreflightConfig::from_env()
    };
    m80_preflight::run_with_configs(
        m80_preflight::BinaryDiscoveryConfig::from_env(),
        artifact_config,
        m80_preflight::HostFeaturePreflightConfig {
            cgroup_mode: m80_preflight::CgroupPreflightMode::Disabled,
        },
    )
    .expect("preflight for explicit image-kind artifacts")
}

fn launch_vm(
    discovery: m80_preflight::Discovery,
    vm_id: &str,
    expected_kind: ImageKind,
) -> m80_firecracker::RunningSandbox {
    assert_eq!(discovery.manifest.image_kind, expected_kind);
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
            vm_id: Some(vm_id.to_owned()),
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
    sandbox.launch().expect("launch")
}

fn exec_sh(running: &mut m80_firecracker::RunningSandbox, script: &str) -> String {
    let response = running
        .exec(ExecRequest {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(10_000),
            streaming: false,
        })
        .expect("exec shell probe");
    assert_eq!(response.status, ExecStatus::Completed);
    assert_eq!(response.exit_code, Some(0));
    String::from_utf8(response.stdout).expect("probe stdout utf8")
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
#[ignore = "requires KVM host with real Minimal and Ubuntu Firecracker artifacts"]
fn image_kind_minimal_boots() {
    let minimal_dir = artifact_dir("M80_MINIMAL_ARTIFACT_DIR", "/tmp/m80-build/minimal");
    let minimal_discovery = discovery_for_artifacts(&minimal_dir);
    let minimal_id = unique_vm_id("ik-minimal");
    let mut minimal = launch_vm(minimal_discovery, &minimal_id, ImageKind::Minimal);
    let minimal_run_dir = minimal.run_dir().to_owned();
    let _minimal_dump_guard = RunDirDumpGuard::new(minimal_run_dir);
    let minimal_stdout = exec_sh(
        &mut minimal,
        "printf 'minimal:'; cat /proc/1/comm; test ! -x /bin/systemctl",
    );
    assert!(
        minimal_stdout.contains("minimal:m80-guestd"),
        "minimal image must run m80-guestd as PID 1, got {minimal_stdout:?}"
    );
    let minimal_stopped = minimal.stop().expect("stop minimal");
    minimal_stopped.delete().expect("delete minimal");
}

#[test]
#[ignore = "requires KVM host with real Ubuntu Firecracker artifacts"]
fn image_kind_ubuntu_boots_systemd() {
    let ubuntu_dir = artifact_dir("M80_UBUNTU_ARTIFACT_DIR", "/tmp/m80-build/ubuntu");
    let ubuntu_discovery = discovery_for_artifacts(&ubuntu_dir);
    let ubuntu_id = unique_vm_id("ik-ubuntu");
    let mut ubuntu = launch_vm(ubuntu_discovery, &ubuntu_id, ImageKind::Ubuntu);
    let ubuntu_run_dir = ubuntu.run_dir().to_owned();
    let _ubuntu_dump_guard = RunDirDumpGuard::new(ubuntu_run_dir);
    let ubuntu_stdout = exec_sh(
        &mut ubuntu,
        "set -eu; printf 'ubuntu:'; cat /proc/1/comm; \
         test -L /etc/systemd/system/basic.target.wants/m80-guestd.service; \
         systemctl is-active --quiet m80-guestd.service; printf ':service-ok'; \
         i=0; while [ \"$i\" -lt 120 ]; do \
           if systemctl is-active --quiet multi-user.target; then \
             printf ':multi-user-ok'; exit 0; \
           fi; \
           i=$((i + 1)); sleep 1; \
         done; \
         systemctl is-active multi-user.target; exit 1",
    );
    assert!(
        ubuntu_stdout.contains("ubuntu:systemd"),
        "ubuntu image must boot systemd as PID 1, got {ubuntu_stdout:?}"
    );
    assert!(
        ubuntu_stdout.contains(":service-ok"),
        "ubuntu image must run m80-guestd as an active basic.target service, got {ubuntu_stdout:?}"
    );
    assert!(
        ubuntu_stdout.contains(":multi-user-ok"),
        "ubuntu image must reach multi-user.target, got {ubuntu_stdout:?}"
    );
    let ubuntu_stopped = ubuntu.stop().expect("stop ubuntu");
    ubuntu_stopped.delete().expect("delete ubuntu");
}
