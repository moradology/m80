use std::path::PathBuf;

use m80_firecracker::EffectiveConfig;
use serde::Serialize;

use crate::profile;

#[derive(Serialize)]
pub(super) struct EnvDump {
    pub(super) version: u16,
    pub(super) cli_version: &'static str,
    pub(super) protocol_version: u32,
    pub(super) host: HostDump,
    pub(super) config: ConfigDump,
    pub(super) runtime_profile: RuntimeProfileDump,
    pub(super) artifacts: ArtifactDump,
    pub(super) firecracker: BinaryDump,
    pub(super) run_root: RunRootDump,
    pub(super) preflight: PreflightDump,
}

#[derive(Serialize)]
pub(super) struct HostDump {
    pub(super) kernel_version: Option<String>,
    pub(super) kvm: DeviceCheck,
    pub(super) vsock: ModuleCheck,
    pub(super) cpu_count: Option<usize>,
    pub(super) kvm_cpu_flags: Vec<String>,
    pub(super) total_memory_kib: Option<u64>,
}

#[derive(Serialize)]
pub(super) struct DeviceCheck {
    pub(super) path: &'static str,
    pub(super) exists: bool,
    pub(super) read_write: bool,
    pub(super) error: Option<String>,
}

#[derive(Serialize)]
pub(super) struct ModuleCheck {
    pub(super) loaded_or_available: bool,
    pub(super) modules: Vec<String>,
}

#[derive(Serialize)]
pub(super) struct ConfigDump {
    pub(super) ok: bool,
    pub(super) effective: Option<EffectiveConfig>,
    pub(super) error: Option<String>,
}

#[derive(Serialize)]
pub(super) struct RuntimeProfileDump {
    pub(super) ok: bool,
    pub(super) name: Option<String>,
    pub(super) selection_source: Option<String>,
    pub(super) body_source: Option<&'static str>,
    pub(super) file_path: Option<PathBuf>,
    pub(super) artifact_dir: Option<PathBuf>,
    pub(super) kernel_image: Option<PathBuf>,
    pub(super) rootfs_image: Option<PathBuf>,
    pub(super) kernel_kind: Option<String>,
    pub(super) guestd: Option<PathBuf>,
    pub(super) guest_manifest: Option<PathBuf>,
    pub(super) build_receipt: Option<PathBuf>,
    pub(super) install_provenance: Option<PathBuf>,
    pub(super) host_binaries_manifest: Option<PathBuf>,
    pub(super) firecracker_bin: Option<PathBuf>,
    pub(super) firecracker_seccomp_filter: Option<PathBuf>,
    pub(super) jailer_bin: Option<PathBuf>,
    pub(super) jailer_harden_bin: Option<PathBuf>,
    pub(super) net_helper_bin: Option<PathBuf>,
    pub(super) run_root: Option<PathBuf>,
    pub(super) release_tag: Option<String>,
    pub(super) m80_version: Option<String>,
    pub(super) description: Option<String>,
    pub(super) active_pointer: Option<PathBuf>,
    pub(super) active_pointer_target: Option<PathBuf>,
    pub(super) active_pointer_status: Option<&'static str>,
    pub(super) active_pointer_error: Option<String>,
    pub(super) missing_paths: Vec<profile::RuntimeProfilePathIssue>,
    pub(super) error: Option<String>,
}

#[derive(Serialize)]
pub(super) struct ArtifactDump {
    pub(super) kernel_image: Option<PathBuf>,
    pub(super) rootfs_image: Option<PathBuf>,
    pub(super) kernel_kind: Option<String>,
    pub(super) rootfs_manifest_path: Option<PathBuf>,
    pub(super) rootfs_manifest_ok: bool,
    pub(super) rootfs_manifest_error: Option<String>,
}

#[derive(Serialize)]
pub(super) struct BinaryDump {
    pub(super) path: PathBuf,
    pub(super) exists: bool,
    pub(super) seccomp_filter_path: PathBuf,
    pub(super) seccomp_filter_exists: bool,
    pub(super) version_output: Option<String>,
    pub(super) error: Option<String>,
    pub(super) configured_pin: Option<String>,
}

#[derive(Serialize)]
pub(super) struct RunRootDump {
    pub(super) path: Option<PathBuf>,
    pub(super) exists: bool,
    pub(super) run_dir_count: Option<usize>,
    pub(super) error: Option<String>,
}

#[derive(Serialize)]
pub(super) struct PreflightDump {
    pub(super) ok: bool,
    pub(super) error: Option<String>,
    pub(super) host_prerequisite_failure: Option<m80_preflight::HostPrerequisiteCheck>,
    pub(super) checks: Vec<m80_preflight::CheckRow>,
}
