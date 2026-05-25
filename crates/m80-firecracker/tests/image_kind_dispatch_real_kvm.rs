//! Real-KVM image-kind dispatch coverage for Minimal and Ubuntu artifacts.

mod common;

use std::path::{Path, PathBuf};

use m80_firecracker::{Backend, BackendConfig, CgroupMode, NetworkPolicy, SandboxConfig};
use m80_image_manifest::{ImageKind, RootfsFormat};
use m80_proto::{ExecRequest, ExecStatus};

use common::RunDirDumpGuard;

fn artifact_dir(env_key: &str, default_path: &str) -> PathBuf {
    std::env::var_os(env_key)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default_path))
}

fn discovery_for_artifacts(dir: &Path) -> m80_preflight::Discovery {
    discovery_for_artifacts_with_rootfs(dir, dir.join("output.ext4"))
}

fn discovery_for_artifacts_with_rootfs(
    dir: &Path,
    rootfs_image: PathBuf,
) -> m80_preflight::Discovery {
    discovery_for_artifacts_with_rootfs_and_kernel(dir, rootfs_image, dir.join("vmlinux"), None)
}

fn discovery_for_artifacts_with_rootfs_and_kernel(
    dir: &Path,
    rootfs_image: PathBuf,
    kernel_image: PathBuf,
    kernel_kind: Option<String>,
) -> m80_preflight::Discovery {
    let artifact_config = m80_preflight::ArtifactPreflightConfig {
        kernel_image: Some(kernel_image),
        artifact_dir: dir.to_owned(),
        rootfs_image: Some(rootfs_image),
        kernel_kind,
        ..m80_preflight::ArtifactPreflightConfig::from_env()
    };
    m80_preflight::run_with_configs(
        m80_preflight::BinaryDiscoveryConfig::from_env(),
        artifact_config,
        m80_preflight::HostFeaturePreflightConfig {
            cgroup_mode: m80_preflight::CgroupPreflightMode::Disabled,
            jail_uid: 3000,
            jail_gid: 3000,
            expected_concurrent_vms: 1,
        },
    )
    .expect("preflight for explicit image-kind artifacts")
}

fn stripped_erofs_kernel_image() -> PathBuf {
    if let Some(path) = std::env::var_os("M80_MINIMAL_EROFS_KERNEL") {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("M80_STRIPPED_KERNEL_PATH") {
        return PathBuf::from(path);
    }
    let kernel_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("m80-firecracker lives under <repo>/crates/m80-firecracker")
        .join("crates/m80-image-build/kernels");
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(&kernel_dir)
        .expect("read stripped kernel directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("vmlinux-m80-") && name.ends_with(".bin"))
        })
        .collect();
    candidates.sort_by_key(|path| path.metadata().and_then(|meta| meta.modified()).ok());
    candidates
        .pop()
        .expect("minimal-erofs real-KVM test requires a built stripped kernel")
}

fn launch_vm(
    discovery: m80_preflight::Discovery,
    vm_id: &str,
    expected_kind: ImageKind,
) -> m80_firecracker::RunningSandbox {
    launch_vm_with_workspace(discovery, vm_id, expected_kind, None)
}

fn launch_vm_with_workspace(
    discovery: m80_preflight::Discovery,
    vm_id: &str,
    expected_kind: ImageKind,
    workspace: Option<PathBuf>,
) -> m80_firecracker::RunningSandbox {
    assert_eq!(discovery.manifest.image_kind, expected_kind);
    let run_root = discovery.run_root.clone();
    let config = BackendConfig::builder(discovery)
        .max_concurrent_vms(1)
        .run_root(run_root)
        .jail_uid(3000)
        .jail_gid(3000)
        .cgroup_mode(CgroupMode::Disabled)
        .build();
    let backend = std::sync::Arc::new(Backend::new(config).expect("Backend::new"));
    let sandbox = backend
        .admit(SandboxConfig {
            vm_id: Some(vm_id.to_owned()),
            workspace,
            network: NetworkPolicy::NoEgress,
            vcpu_count: Some(1),
            mem_size_mib: Some(512),
            huge_pages_2m: false,
            cpuset_cpus: None,
            cpu_template: None,
            fc_log_level: None,
            drive_cache_type: None,
            boot_args: None,
            overlay_size_bytes: 512 * 1024 * 1024,
            overlay_clone_mode: Default::default(),
            idle_timeout: None,
            max_lifetime: None,
            daemonize: false,
            request_id: None,
            pmem_layers: Vec::new(),
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

#[test]
#[ignore = "requires KVM host with real Minimal and Ubuntu Firecracker artifacts"]
fn image_kind_minimal_boots() {
    let minimal_dir = artifact_dir("M80_MINIMAL_ARTIFACT_DIR", "/tmp/m80-build/minimal");
    let minimal_discovery = discovery_for_artifacts(&minimal_dir);
    let minimal_id = common::unique_vm_id("ik-minimal");
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
#[ignore = "requires KVM host with real Minimal erofs Firecracker artifacts"]
fn image_kind_minimal_erofs_boots() {
    let erofs_dir = artifact_dir(
        "M80_MINIMAL_EROFS_ARTIFACT_DIR",
        "/tmp/m80-build/minimal-erofs",
    );
    let erofs_discovery = discovery_for_artifacts_with_rootfs_and_kernel(
        &erofs_dir,
        erofs_dir.join("output.erofs"),
        stripped_erofs_kernel_image(),
        Some("stripped".to_owned()),
    );
    assert_eq!(erofs_discovery.manifest.rootfs_format, RootfsFormat::Erofs);
    let erofs_id = common::unique_vm_id("ik-min-erofs");
    let mut erofs = launch_vm(erofs_discovery, &erofs_id, ImageKind::Minimal);
    let erofs_run_dir = erofs.run_dir().to_owned();
    let _erofs_dump_guard = RunDirDumpGuard::new(erofs_run_dir);
    let erofs_stdout = exec_sh(
        &mut erofs,
        "printf 'minimal-erofs:'; cat /proc/1/comm; \
         awk '$2 == \"/lower\" { print $3 }' /proc/mounts",
    );
    assert!(
        erofs_stdout.contains("minimal-erofs:m80-guestd"),
        "minimal-erofs image must run m80-guestd as PID 1, got {erofs_stdout:?}"
    );
    assert!(
        erofs_stdout.contains("erofs"),
        "minimal-erofs lower mount must use erofs, got {erofs_stdout:?}"
    );
    let erofs_stopped = erofs.stop().expect("stop minimal erofs");
    erofs_stopped.delete().expect("delete minimal erofs");
}

#[test]
#[ignore = "requires KVM host with real Ubuntu Firecracker artifacts"]
fn image_kind_ubuntu_boots_guestd_as_pid_one() {
    let ubuntu_dir = artifact_dir("M80_UBUNTU_ARTIFACT_DIR", "/tmp/m80-build/ubuntu");
    let ubuntu_discovery = discovery_for_artifacts(&ubuntu_dir);
    let ubuntu_id = common::unique_vm_id("ik-ubuntu");
    let mut ubuntu = launch_vm(ubuntu_discovery, &ubuntu_id, ImageKind::Ubuntu);
    let ubuntu_run_dir = ubuntu.run_dir().to_owned();
    let _ubuntu_dump_guard = RunDirDumpGuard::new(ubuntu_run_dir);
    let ubuntu_stdout = exec_sh(
        &mut ubuntu,
        "set -eu; printf 'ubuntu:'; cat /proc/1/comm; \
         mount | grep 'overlay on / type overlay' >/dev/null; \
         test ! -e /etc/systemd/system/basic.target.wants/m80-guestd.service; \
         printf ':overlay-ok'",
    );
    assert!(
        ubuntu_stdout.contains("ubuntu:m80-guestd"),
        "ubuntu image must run m80-guestd as PID 1, got {ubuntu_stdout:?}"
    );
    assert!(
        ubuntu_stdout.contains(":overlay-ok"),
        "ubuntu image must use the PID-1 overlay pivot, got {ubuntu_stdout:?}"
    );
    let ubuntu_stopped = ubuntu.stop().expect("stop ubuntu");
    ubuntu_stopped.delete().expect("delete ubuntu");
}

#[test]
#[ignore = "requires KVM host with real Minimal and Ubuntu Firecracker artifacts"]
fn overlay_assembly_per_image_kind() {
    for (label, env_key, default_path, kind) in [
        (
            "minimal",
            "M80_MINIMAL_ARTIFACT_DIR",
            "/tmp/m80-build/minimal",
            ImageKind::Minimal,
        ),
        (
            "ubuntu",
            "M80_UBUNTU_ARTIFACT_DIR",
            "/tmp/m80-build/ubuntu",
            ImageKind::Ubuntu,
        ),
    ] {
        let artifact_dir = artifact_dir(env_key, default_path);
        let discovery = discovery_for_artifacts(&artifact_dir);
        let vm_id = common::unique_vm_id(&format!("ik-overlay-{label}"));
        let workspace = tempfile::tempdir().expect("workspace tempdir");
        let mut running = launch_vm_with_workspace(
            discovery,
            &vm_id,
            kind,
            Some(workspace.path().to_path_buf()),
        );
        let run_dir = running.run_dir().to_owned();
        let _dump_guard = RunDirDumpGuard::new(run_dir);

        let stdout = exec_sh(
            &mut running,
            "set -eu; \
             test -r /lower; test -w /upper; test -r /merged; \
             mount | grep 'overlay on / type overlay' >/dev/null; \
             awk '$2 == \"/workspace\" { print $1, $2, $3 }' /proc/mounts; \
             printf ok > /workspace/overlay-kind.txt; \
             cat /workspace/overlay-kind.txt",
        );
        assert!(
            stdout.contains("/dev/vdc /workspace ext4"),
            "{label} workspace must mount from the third Firecracker drive:\n{stdout}"
        );
        assert!(
            stdout.trim_end().ends_with("ok"),
            "{label} workspace write/read probe failed:\n{stdout}"
        );

        let stopped = running.stop().expect("stop image-kind overlay VM");
        stopped.delete().expect("delete image-kind overlay VM");
    }
}
