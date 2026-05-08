//! The ordered preflight checks that populate a [`Discovery`].

use std::fs;
use std::io;
use std::path::PathBuf;

use caps::CapSet;
use nix::sys::utsname::uname;
use nix::unistd::geteuid;

use crate::{
    classify_privilege, discover_binaries, verify_artifacts, ArtifactPreflightConfig,
    BinaryDiscoveryConfig, CheckRow, Discovery, PreflightError, PrivilegeStatus,
    REQUIRED_CAPABILITIES,
};

const KVM_PATH: &str = "/dev/kvm";
const VHOST_VSOCK_PATH: &str = "/dev/vhost-vsock";

const REQUIRED_KERNEL_MODULES: &[&str] = &["tap", "bridge"];

/// Run all checks in order, deriving binary and artifact config from env.
///
/// This is the zero-argument convenience entry point. Callers that have
/// already resolved an effective config (e.g. from a TOML file) should use
/// [`run_with_configs`] so that TOML-set fields like `run_root` are honoured.
pub fn run() -> Result<Discovery, PreflightError> {
    run_with_configs(
        BinaryDiscoveryConfig::from_env(),
        ArtifactPreflightConfig::from_env(),
    )
}

/// Run all checks in order with explicit binary and artifact configs.
///
/// Use this when the caller has already resolved the effective configuration
/// (e.g. from `/etc/m80/config.toml` or `~/.config/m80/config.toml`) and
/// needs preflight to validate the same paths that will be used at runtime.
/// Fields not present in the effective config (e.g. `firecracker_bin`,
/// `kernel_image`) are still read from env inside the individual `from_env`
/// constructors; only the fields that the caller overrides here win.
pub fn run_with_configs(
    binary_config: BinaryDiscoveryConfig,
    artifact_config: ArtifactPreflightConfig,
) -> Result<Discovery, PreflightError> {
    let mut report: Vec<CheckRow> = Vec::new();

    // 1. OS gate
    check_os(&mut report)?;

    // 2. KVM
    check_kvm(&mut report)?;

    // 3. KVM CPU extensions
    check_kvm_cpu_extensions(&mut report)?;

    // 4. Kernel modules
    check_kernel_modules(&mut report)?;

    // 5. Privilege
    let privilege = check_privilege(&mut report)?;

    // 6-8. Firecracker and jailer binaries
    let binaries = discover_binaries(&binary_config)?;
    report.push(CheckRow {
        label: "Firecracker binary".to_string(),
        passed: true,
        detail: format!(
            "{} ({})",
            binaries.firecracker_bin.display(),
            binaries.firecracker_version
        ),
    });

    report.push(CheckRow {
        label: "Jailer binary".to_string(),
        passed: true,
        detail: binaries.jailer_bin.display().to_string(),
    });
    report.push(CheckRow {
        label: "Jailer hardening wrapper".to_string(),
        passed: true,
        detail: binaries.jailer_harden_bin.display().to_string(),
    });

    // 9-12. Kernel/rootfs artifacts, run-root, and storage helpers
    let artifacts = verify_artifacts(&artifact_config)?;
    report.push(CheckRow {
        label: "Kernel image".to_string(),
        passed: true,
        detail: artifacts.kernel.display().to_string(),
    });

    report.push(CheckRow {
        label: "Rootfs + manifest".to_string(),
        passed: true,
        detail: format!("{} (sha256 ok)", artifacts.rootfs.display()),
    });

    report.push(CheckRow {
        label: "Run-root".to_string(),
        passed: true,
        detail: artifacts.run_root.display().to_string(),
    });

    report.push(CheckRow {
        label: "Storage helpers".to_string(),
        passed: true,
        detail: artifacts.storage_helpers.join(", "),
    });

    Ok(Discovery {
        firecracker_bin: binaries.firecracker_bin,
        jailer_bin: binaries.jailer_bin,
        jailer_harden_bin: binaries.jailer_harden_bin,
        kernel: artifacts.kernel,
        rootfs: artifacts.rootfs,
        manifest: artifacts.manifest,
        run_root: artifacts.run_root,
        privilege,
        report,
    })
}

fn check_os(report: &mut Vec<CheckRow>) -> Result<(), PreflightError> {
    let uts = uname().map_err(|e| PreflightError::Io(e.into()))?;
    let sysname = uts.sysname().to_string_lossy().into_owned();
    if sysname != "Linux" {
        return Err(PreflightError::UnsupportedHostPlatform { actual: sysname });
    }
    let release = uts.release().to_string_lossy().into_owned();
    report.push(CheckRow {
        label: "OS gate".to_string(),
        passed: true,
        detail: format!("Linux {release}"),
    });
    Ok(())
}

fn check_kvm(report: &mut Vec<CheckRow>) -> Result<(), PreflightError> {
    let kvm = PathBuf::from(KVM_PATH);

    let exists = kvm.exists();

    // Write-access check: open O_WRONLY; close immediately.
    // EACCES → permission denied → fail; other errors (EBUSY etc.) are not
    // access-denial and we don't block on them.
    let write_open = fs::OpenOptions::new().write(true).open(&kvm).map(drop);
    classify_kvm_access(&kvm, exists, write_open)?;

    report.push(CheckRow {
        label: "KVM".to_string(),
        passed: true,
        detail: format!("{} present and writable", kvm.display()),
    });
    Ok(())
}

fn classify_kvm_access(
    path: &std::path::Path,
    exists: bool,
    write_open: Result<(), io::Error>,
) -> Result<(), PreflightError> {
    if !exists {
        return Err(PreflightError::KvmUnavailable {
            path: path.to_path_buf(),
        });
    }

    match write_open {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            Err(PreflightError::KvmNotWritable {
                path: path.to_path_buf(),
            })
        }
        Err(_) => Ok(()),
    }
}

fn check_kvm_cpu_extensions(report: &mut Vec<CheckRow>) -> Result<(), PreflightError> {
    let cpuinfo = fs::read_to_string("/proc/cpuinfo").map_err(PreflightError::Io)?;
    let flags = classify_kvm_cpu_flags(&cpuinfo)?;

    report.push(CheckRow {
        label: "KVM CPU extensions".to_string(),
        passed: true,
        detail: flags.join(", "),
    });
    Ok(())
}

fn classify_kvm_cpu_flags(cpuinfo: &str) -> Result<Vec<String>, PreflightError> {
    let mut flags = Vec::new();
    for line in cpuinfo.lines() {
        let Some(rest) = line.strip_prefix("flags") else {
            continue;
        };
        let Some((_, values)) = rest.split_once(':') else {
            continue;
        };
        for flag in values.split_whitespace() {
            if matches!(flag, "vmx" | "svm") && !flags.iter().any(|seen| seen == flag) {
                flags.push(flag.to_owned());
            }
        }
    }

    if flags.is_empty() {
        return Err(PreflightError::KvmCpuExtensionMissing);
    }
    Ok(flags)
}

fn check_kernel_modules(report: &mut Vec<CheckRow>) -> Result<(), PreflightError> {
    let content = fs::read_to_string("/proc/modules").map_err(PreflightError::Io)?;
    let loaded: Vec<&str> = content
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect();

    classify_vsock_availability(&loaded, std::path::Path::new(VHOST_VSOCK_PATH).exists())?;
    classify_required_modules(&loaded)?;

    report.push(CheckRow {
        label: "Kernel modules".to_string(),
        passed: true,
        detail: "tap, bridge loaded; vhost-vsock available".to_string(),
    });
    Ok(())
}

fn classify_vsock_availability(
    loaded_modules: &[&str],
    vhost_vsock_device_exists: bool,
) -> Result<(), PreflightError> {
    if loaded_modules.contains(&"vhost_vsock") || vhost_vsock_device_exists {
        return Ok(());
    }
    Err(PreflightError::VsockUnavailable)
}

fn classify_required_modules(loaded: &[&str]) -> Result<(), PreflightError> {
    if REQUIRED_KERNEL_MODULES.iter().any(|m| !loaded.contains(m)) {
        let missing = REQUIRED_KERNEL_MODULES
            .iter()
            .filter(|m| !loaded.contains(*m))
            .map(|m| m.to_string())
            .collect::<Vec<_>>();
        return Err(PreflightError::KernelModulesMissing { missing });
    }
    Ok(())
}

fn check_privilege(report: &mut Vec<CheckRow>) -> Result<PrivilegeStatus, PreflightError> {
    let euid = geteuid().as_raw();
    let effective = caps::read(None, CapSet::Effective).map_err(PreflightError::CapabilityRead)?;
    let status = classify_privilege(euid, &effective)?;

    report.push(CheckRow {
        label: "Privilege".to_string(),
        passed: true,
        detail: match status {
            PrivilegeStatus::Root => "euid == 0 (root)".to_string(),
            PrivilegeStatus::CapabilityBearing => format!(
                "capability-bearing ({})",
                REQUIRED_CAPABILITIES
                    .iter()
                    .map(|c| format!("{c:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        },
    });
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preflight_missing_vsock_module_typed() {
        let err = classify_vsock_availability(&["tap", "bridge"], false).unwrap_err();

        assert!(matches!(err, PreflightError::VsockUnavailable));
    }

    #[test]
    fn vhost_vsock_module_satisfies_vsock_preflight() {
        classify_vsock_availability(&["tap", "bridge", "vhost_vsock"], false).unwrap();
    }

    #[test]
    fn vhost_vsock_device_satisfies_vsock_preflight() {
        classify_vsock_availability(&["tap", "bridge"], true).unwrap();
    }

    #[test]
    fn required_kernel_modules_report_missing_tap_bridge() {
        let err = classify_required_modules(&["vhost_vsock"]).unwrap_err();

        match err {
            PreflightError::KernelModulesMissing { missing } => {
                assert_eq!(missing, vec!["tap".to_owned(), "bridge".to_owned()]);
            }
            other => panic!("expected KernelModulesMissing, got {other:?}"),
        }
    }

    #[test]
    fn preflight_missing_kvm_returns_typed_hint() {
        let path = PathBuf::from("/dev/kvm");

        let err = classify_kvm_access(&path, false, Ok(())).unwrap_err();

        match err {
            PreflightError::KvmUnavailable { path: actual } => {
                assert_eq!(actual, path);
                assert!(
                    err_hint_mentions_kvm_enable(&PreflightError::KvmUnavailable { path: actual }),
                    "hint must point at enabling KVM"
                );
            }
            other => panic!("expected KvmUnavailable, got {other:?}"),
        }
    }

    #[test]
    fn preflight_kvm_permission_denied_typed() {
        let path = PathBuf::from("/dev/kvm");

        let err = classify_kvm_access(
            &path,
            true,
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied")),
        )
        .unwrap_err();

        match err {
            PreflightError::KvmNotWritable { path: actual } => assert_eq!(actual, path),
            other => panic!("expected KvmNotWritable, got {other:?}"),
        }
    }

    #[test]
    fn non_access_kvm_open_errors_do_not_block_preflight() {
        let result = classify_kvm_access(
            PathBuf::from("/dev/kvm").as_path(),
            true,
            Err(io::Error::new(io::ErrorKind::Other, "busy")),
        );

        assert!(result.is_ok());
    }

    #[test]
    fn preflight_missing_vmx_svm_returns_typed() {
        let cpuinfo = "\
processor\t: 0
vendor_id\t: GenuineIntel
flags\t\t: fpu tsc msr pae
";

        let err = classify_kvm_cpu_flags(cpuinfo).unwrap_err();

        assert!(matches!(err, PreflightError::KvmCpuExtensionMissing));
    }

    #[test]
    fn preflight_vmx_svm_flags_are_reported_once() {
        let cpuinfo = "\
processor\t: 0
flags\t\t: fpu vmx tsc vmx
processor\t: 1
flags\t\t: fpu svm tsc
";

        let flags = classify_kvm_cpu_flags(cpuinfo).unwrap();

        assert_eq!(flags, vec!["vmx", "svm"]);
    }

    fn err_hint_mentions_kvm_enable(err: &PreflightError) -> bool {
        let hint = err.hint();
        hint.contains("KVM") && hint.contains("enabled")
    }
}
