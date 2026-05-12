//! Real-KVM adversarial guestd tests.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use m80_firecracker::{Backend, BackendConfig, CgroupMode, NetworkPolicy, SandboxConfig};

#[path = "malicious/bogus_request_id.rs"]
mod bogus_request_id;
#[path = "malicious/dos_attacks.rs"]
mod dos_attacks;
#[path = "malicious/oversized_length.rs"]
mod oversized_length;
#[path = "malicious/response_type_mismatch.rs"]
mod response_type_mismatch;
#[path = "malicious/truncated_frame.rs"]
mod truncated_frame;
#[path = "malicious/unknown_variant.rs"]
mod unknown_variant;
#[path = "malicious/unsolicited_response.rs"]
mod unsolicited_response;

fn launch_malicious(
    attack: &str,
) -> (
    Arc<Backend>,
    m80_firecracker::RunningSandbox,
    std::path::PathBuf,
) {
    let artifact_dir = std::env::var_os("M80_MALICIOUS_ARTIFACT_DIR")
        .map(PathBuf::from)
        .expect("M80_MALICIOUS_ARTIFACT_DIR must point at malicious guestd image artifacts");
    let discovery = discovery_for_artifacts(&artifact_dir);
    let run_root = discovery.run_root.clone();
    let backend = Arc::new(
        Backend::new(BackendConfig {
            discovery,
            max_concurrent_vms: 1,
            run_root,
            jail_uid: 3000,
            jail_gid: 3000,
            cgroup_mode: CgroupMode::Disabled,
        })
        .expect("Backend::new"),
    );
    let vm_id = common::unique_vm_id(&format!("malicious-{attack}"));
    let sandbox = backend
        .admit(SandboxConfig {
            vm_id: Some(vm_id),
            workspace: None,
            network: NetworkPolicy::NoEgress,
            vcpu_count: Some(1),
            mem_size_mib: Some(512),
            boot_args: Some(format!(
                "console=ttyS0 reboot=k panic=-1 pci=off init=/m80-guestd \
                 m80.malicious_attack={attack}"
            )),
            overlay_size_bytes: 512 * 1024 * 1024,
            idle_timeout: None,
            daemonize: false,
            request_id: None,
            preallocated_drive_slots: 0,
            one_shot: false,
        })
        .expect("admit malicious VM");

    let running = sandbox.launch().expect("malicious guestd reaches ready");
    let run_dir = running.run_dir().to_path_buf();
    (backend, running, run_dir)
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

