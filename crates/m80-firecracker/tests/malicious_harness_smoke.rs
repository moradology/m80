//! Real-KVM smoke for the test-only malicious guestd artifact.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::RunDirDumpGuard;
use m80_firecracker::{Backend, BackendConfig, CgroupMode, NetworkPolicy, SandboxConfig};

#[test]
#[ignore = "requires KVM host and M80_MALICIOUS_ARTIFACT_DIR image built with m80-guestd-malicious"]
fn malicious_guestd_noop_reaches_ready_signal() {
    let artifact_dir = std::env::var_os("M80_MALICIOUS_ARTIFACT_DIR")
        .map(PathBuf::from)
        .expect("M80_MALICIOUS_ARTIFACT_DIR must point at malicious guestd image artifacts");
    let discovery = discovery_for_artifacts(&artifact_dir);
    let run_root = discovery.run_root.clone();
    let backend = Arc::new(
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
    let vm_id = common::unique_vm_id("malicious-noop");
    let sandbox = backend
        .admit(SandboxConfig {
            vm_id: Some(vm_id),
            workspace: None,
            network: NetworkPolicy::NoEgress,
            vcpu_count: Some(1),
            mem_size_mib: Some(512),
            cpuset_cpus: None,
            cpu_template: None,
            drive_cache_type: None,
            boot_args: Some("m80.malicious_attack=noop".into()),
            overlay_size_bytes: 512 * 1024 * 1024,
            idle_timeout: None,
            daemonize: false,
            request_id: None,
            preallocated_drive_slots: 0,
            one_shot: false,
        })
        .expect("admit malicious noop");

    let running = sandbox
        .launch()
        .expect("malicious noop guestd reaches host ready signal");
    let _dump_guard = RunDirDumpGuard::new(running.run_dir().to_path_buf());
    running
        .force_kill()
        .expect("force kill malicious noop VM")
        .delete()
        .expect("delete malicious noop VM");
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
    .expect("preflight for malicious guestd artifacts")
}
