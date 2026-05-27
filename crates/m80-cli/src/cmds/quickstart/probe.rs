use std::path::Path;
use std::process::Command;

use m80_firecracker::FcError;

pub(super) const RUN_ECHO_PROBE_COMMAND: &str = "m80 run -- echo hello";
const RUN_ECHO_PROBE_ARGS: &[&str] = &["run", "--", "echo", "hello"];
pub(super) const RUN_ECHO_PROBE_EGRESS_POLICY: &str = "default-outbound";
const RUN_ECHO_PROBE_ENV_REMOVALS: &[&str] = &[
    "M80_ARTIFACT_DIR",
    "M80_KERNEL_IMAGE",
    "M80_ROOTFS_IMAGE",
    "M80_KERNEL_KIND",
    "M80_RUN_ROOT",
    "M80_DEFAULT_PROFILE",
    "M80_MAX_CONCURRENT_VMS",
    "M80_JAIL_UID",
    "M80_JAIL_GID",
    "M80_CGROUP_MODE",
    "M80_FIRECRACKER_BIN",
    "M80_FIRECRACKER_VERSION",
    "M80_FIRECRACKER_SECCOMP_FILTER",
    "M80_JAILER_BIN",
    "M80_JAILER_HARDEN_BIN",
    "M80_NET_HELPER_BIN",
    "M80_SKIP_CHECK_VULNERABILITIES",
    "M80_FORCE_PREFLIGHT",
    "M80_PHASE_TRACE",
];

pub(super) fn write_host_binaries_manifest_for_probe(artifact_dir: &Path) -> Result<bool, FcError> {
    let current = std::env::current_exe()
        .map_err(|source| crate::errors::host_io("resolve current executable", source))?;
    let mut config = m80_preflight::HostBinariesManifestConfig::from_env();
    config.m80_bin = current;
    config.include_jailer_harden = include_jailer_harden_for_probe()?;
    let manifest_path = artifact_dir.join("host-binaries.manifest.json");
    m80_preflight::write_host_binaries_manifest(&config, &manifest_path)?;
    Ok(config.include_jailer_harden)
}

pub(super) fn include_jailer_harden_for_probe() -> Result<bool, FcError> {
    Ok(
        m80_preflight::select_host_launch_path(&m80_preflight::BinaryDiscoveryConfig::from_env())?
            == m80_preflight::LaunchPath::Wrapper,
    )
}

pub(super) fn run_echo_probe() -> Result<(), FcError> {
    let current = std::env::current_exe()
        .map_err(|source| crate::errors::host_io("resolve current executable", source))?;
    let mut command = Command::new(current);
    command.args(RUN_ECHO_PROBE_ARGS);
    for key in RUN_ECHO_PROBE_ENV_REMOVALS {
        command.env_remove(key);
    }
    let status = command
        .status()
        .map_err(|source| FcError::CommandSpawnFailed {
            command: RUN_ECHO_PROBE_COMMAND,
            source,
        })?;
    if !status.success() {
        return Err(FcError::CommandFailed {
            command: RUN_ECHO_PROBE_COMMAND,
            status,
            output: String::new(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        RUN_ECHO_PROBE_ARGS, RUN_ECHO_PROBE_COMMAND, RUN_ECHO_PROBE_EGRESS_POLICY,
        RUN_ECHO_PROBE_ENV_REMOVALS,
    };

    #[test]
    fn echo_probe_command_is_plain_public_target() {
        assert_eq!(RUN_ECHO_PROBE_COMMAND, "m80 run -- echo hello");
        assert_eq!(RUN_ECHO_PROBE_ARGS, ["run", "--", "echo", "hello"]);
        assert!(!RUN_ECHO_PROBE_ARGS.contains(&"--egress"));
        assert_eq!(RUN_ECHO_PROBE_EGRESS_POLICY, "default-outbound");
    }

    #[test]
    fn echo_probe_scrubs_runtime_env_overrides() {
        for key in [
            "M80_ARTIFACT_DIR",
            "M80_KERNEL_IMAGE",
            "M80_ROOTFS_IMAGE",
            "M80_KERNEL_KIND",
            "M80_RUN_ROOT",
            "M80_DEFAULT_PROFILE",
            "M80_MAX_CONCURRENT_VMS",
            "M80_JAIL_UID",
            "M80_JAIL_GID",
            "M80_CGROUP_MODE",
            "M80_FIRECRACKER_BIN",
            "M80_FIRECRACKER_VERSION",
            "M80_FIRECRACKER_SECCOMP_FILTER",
            "M80_JAILER_BIN",
            "M80_JAILER_HARDEN_BIN",
            "M80_NET_HELPER_BIN",
            "M80_SKIP_CHECK_VULNERABILITIES",
            "M80_FORCE_PREFLIGHT",
            "M80_PHASE_TRACE",
        ] {
            assert!(
                RUN_ECHO_PROBE_ENV_REMOVALS.contains(&key),
                "quickstart probe must remove ambient {key}"
            );
        }
    }
}
