//! The ordered preflight checks that populate a [`Discovery`].

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::PathBuf;

use caps::CapSet;
use nix::sys::utsname::uname;
use nix::unistd::geteuid;

use crate::artifacts::{verify_artifacts, ArtifactPreflightConfig};
use crate::binary::{discover_binaries, BinaryDiscoveryConfig};
use crate::{
    classify_privilege, CheckRow, Discovery, PreflightError, PrivilegeStatus, REQUIRED_CAPABILITIES,
};

const KVM_PATH: &str = "/dev/kvm";
const NF_CONNTRACK_MODULE_PATH: &str = "/sys/module/nf_conntrack";
const TUN_PATH: &str = "/dev/net/tun";
const VHOST_VSOCK_PATH: &str = "/dev/vhost-vsock";

const REQUIRED_KERNEL_MODULES: &[&str] = &["tap", "bridge"];

/// Cgroup mode preflight should validate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CgroupPreflightMode {
    /// Caller will not use m80's cgroup v2 subtree support.
    Disabled,
    /// Caller will use m80's cgroup v2 subtree support.
    UnifiedV2,
}

/// Host feature knobs whose required checks depend on effective config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostFeaturePreflightConfig {
    /// Cgroup mode to validate.
    pub cgroup_mode: CgroupPreflightMode,
}

impl HostFeaturePreflightConfig {
    /// Build from exact m80 env vars. Absent `M80_CGROUP_MODE` defaults to
    /// `unified-v2`, matching `m80-firecracker` config defaults.
    pub fn from_env() -> Result<Self, PreflightError> {
        let cgroup_mode = match std::env::var("M80_CGROUP_MODE") {
            Ok(value) => parse_cgroup_mode(&value)?,
            Err(std::env::VarError::NotPresent) => CgroupPreflightMode::UnifiedV2,
            Err(std::env::VarError::NotUnicode(value)) => {
                return Err(PreflightError::InvalidCgroupMode {
                    actual: value.to_string_lossy().into_owned(),
                });
            }
        };
        Ok(Self { cgroup_mode })
    }
}

fn parse_cgroup_mode(value: &str) -> Result<CgroupPreflightMode, PreflightError> {
    match value {
        "disabled" => Ok(CgroupPreflightMode::Disabled),
        "unified-v2" => Ok(CgroupPreflightMode::UnifiedV2),
        other => Err(PreflightError::InvalidCgroupMode {
            actual: other.to_owned(),
        }),
    }
}

/// Run all checks in order, deriving binary and artifact config from env.
///
/// This is the zero-argument convenience entry point. Callers that have
/// already resolved an effective config (e.g. from a TOML file) should use
/// [`run_with_configs`] so that TOML-set fields like `run_root` are honoured.
pub fn run() -> Result<Discovery, PreflightError> {
    run_with_configs(
        BinaryDiscoveryConfig::from_env(),
        ArtifactPreflightConfig::from_env(),
        HostFeaturePreflightConfig::from_env()?,
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
    host_feature_config: HostFeaturePreflightConfig,
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

    // 5. Cgroup host mode
    check_cgroup_mode(host_feature_config.cgroup_mode, &mut report)?;

    // 6. Privilege
    let privilege = check_privilege(&mut report)?;

    // 7-9. Firecracker and jailer binaries
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

    // 10-13. Kernel/rootfs artifacts, run-root, and storage helpers
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
    let uts = uname().map_err(|e| PreflightError::SystemIo {
        operation: "uname",
        source: e.into(),
    })?;
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
        Err(source) => Err(PreflightError::PathIo {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn check_kvm_cpu_extensions(report: &mut Vec<CheckRow>) -> Result<(), PreflightError> {
    let cpuinfo = fs::read_to_string("/proc/cpuinfo").map_err(|source| PreflightError::PathIo {
        path: PathBuf::from("/proc/cpuinfo"),
        source,
    })?;
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
    let content = fs::read_to_string("/proc/modules").map_err(|source| PreflightError::PathIo {
        path: PathBuf::from("/proc/modules"),
        source,
    })?;
    let loaded: HashSet<&str> = content
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect();

    classify_vsock_availability(&loaded, std::path::Path::new(VHOST_VSOCK_PATH).exists())?;
    classify_tun_availability(&loaded, std::path::Path::new(TUN_PATH).exists())?;
    classify_nf_conntrack_availability(
        &loaded,
        std::path::Path::new(NF_CONNTRACK_MODULE_PATH).exists(),
    )?;
    classify_required_modules(&loaded)?;

    report.push(CheckRow {
        label: "Kernel modules".to_string(),
        passed: true,
        detail: "tap, bridge loaded; tun, vhost-vsock, nf_conntrack available".to_string(),
    });
    Ok(())
}

fn classify_vsock_availability(
    loaded_modules: &HashSet<&str>,
    vhost_vsock_device_exists: bool,
) -> Result<(), PreflightError> {
    if loaded_modules.contains("vhost_vsock") || vhost_vsock_device_exists {
        return Ok(());
    }
    Err(PreflightError::VsockUnavailable)
}

fn classify_tun_availability(
    loaded_modules: &HashSet<&str>,
    tun_device_exists: bool,
) -> Result<(), PreflightError> {
    if loaded_modules.contains("tun") || tun_device_exists {
        return Ok(());
    }
    Err(PreflightError::TunUnavailable)
}

fn classify_nf_conntrack_availability(
    loaded_modules: &HashSet<&str>,
    sys_module_exists: bool,
) -> Result<(), PreflightError> {
    if loaded_modules.contains("nf_conntrack") || sys_module_exists {
        return Ok(());
    }
    Err(PreflightError::NfConntrackUnavailable)
}

fn classify_required_modules(loaded: &HashSet<&str>) -> Result<(), PreflightError> {
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

fn check_cgroup_mode(
    mode: CgroupPreflightMode,
    report: &mut Vec<CheckRow>,
) -> Result<(), PreflightError> {
    check_cgroup_mode_with_probe(mode, report, m80_cgroup::Subtree::probe)
}

fn check_cgroup_mode_with_probe<F>(
    mode: CgroupPreflightMode,
    report: &mut Vec<CheckRow>,
    probe: F,
) -> Result<(), PreflightError>
where
    F: FnOnce() -> Result<(), m80_cgroup::CgroupError>,
{
    if mode == CgroupPreflightMode::UnifiedV2 {
        classify_cgroup_probe(mode, probe())?;
    }
    report.push(CheckRow {
        label: "Cgroup mode".to_string(),
        passed: true,
        detail: match mode {
            CgroupPreflightMode::Disabled => "disabled".to_string(),
            CgroupPreflightMode::UnifiedV2 => "unified-v2 available".to_string(),
        },
    });
    Ok(())
}

fn classify_cgroup_probe(
    mode: CgroupPreflightMode,
    probe: Result<(), m80_cgroup::CgroupError>,
) -> Result<(), PreflightError> {
    if mode == CgroupPreflightMode::Disabled {
        return Ok(());
    }

    match probe {
        Ok(()) => Ok(()),
        Err(m80_cgroup::CgroupError::UnsupportedHostMode) => {
            Err(PreflightError::CgroupV2Unavailable)
        }
        Err(err) => Err(PreflightError::SystemIo {
            operation: "cgroup v2 probe",
            source: std::io::Error::other(format!("{err}")),
        }),
    }
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
    fn cgroup_mode_parser_accepts_disabled() {
        let mode = parse_cgroup_mode("disabled").unwrap();

        assert_eq!(mode, CgroupPreflightMode::Disabled);
    }

    #[test]
    fn cgroup_mode_parser_accepts_unified_v2() {
        let mode = parse_cgroup_mode("unified-v2").unwrap();

        assert_eq!(mode, CgroupPreflightMode::UnifiedV2);
    }

    #[test]
    fn cgroup_mode_parser_rejects_unknown_value() {
        let err = parse_cgroup_mode("legacy").unwrap_err();

        match err {
            PreflightError::InvalidCgroupMode { actual } => assert_eq!(actual, "legacy"),
            other => panic!("expected InvalidCgroupMode, got {other:?}"),
        }
    }

    #[test]
    fn preflight_cgroup_v2_unavailability_typed() {
        let err = classify_cgroup_probe(
            CgroupPreflightMode::UnifiedV2,
            Err(m80_cgroup::CgroupError::UnsupportedHostMode),
        )
        .unwrap_err();

        assert!(matches!(err, PreflightError::CgroupV2Unavailable));
    }

    #[test]
    fn disabled_cgroup_mode_skips_cgroup_v2_probe() {
        classify_cgroup_probe(
            CgroupPreflightMode::Disabled,
            Err(m80_cgroup::CgroupError::UnsupportedHostMode),
        )
        .unwrap();
    }

    #[test]
    fn disabled_cgroup_mode_does_not_call_live_probe() {
        let mut report = Vec::new();

        check_cgroup_mode_with_probe(CgroupPreflightMode::Disabled, &mut report, || {
            panic!("disabled cgroup mode must not probe cgroup v2")
        })
        .unwrap();

        assert_eq!(report.len(), 1);
        assert_eq!(report[0].label, "Cgroup mode");
        assert_eq!(report[0].detail, "disabled");
    }

    #[test]
    fn cgroup_v2_probe_errors_remain_typed_system_io() {
        let err = classify_cgroup_probe(
            CgroupPreflightMode::UnifiedV2,
            Err(m80_cgroup::CgroupError::ControllerNotEnabled("cpu")),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            PreflightError::SystemIo {
                operation: "cgroup v2 probe",
                ..
            }
        ));
    }

    #[test]
    fn preflight_missing_vsock_module_typed() {
        let err =
            classify_vsock_availability(&HashSet::from(["tap", "bridge"]), false).unwrap_err();

        assert!(matches!(err, PreflightError::VsockUnavailable));
    }

    #[test]
    fn vhost_vsock_module_satisfies_vsock_preflight() {
        classify_vsock_availability(&HashSet::from(["tap", "bridge", "vhost_vsock"]), false)
            .unwrap();
    }

    #[test]
    fn vhost_vsock_device_satisfies_vsock_preflight() {
        classify_vsock_availability(&HashSet::from(["tap", "bridge"]), true).unwrap();
    }

    #[test]
    fn preflight_missing_tun_module_typed() {
        let err =
            classify_tun_availability(&HashSet::from(["tap", "bridge"]), false).unwrap_err();

        assert!(matches!(err, PreflightError::TunUnavailable));
    }

    #[test]
    fn tun_module_satisfies_tun_preflight() {
        classify_tun_availability(&HashSet::from(["tap", "bridge", "tun"]), false).unwrap();
    }

    #[test]
    fn tun_device_satisfies_tun_preflight() {
        classify_tun_availability(&HashSet::from(["tap", "bridge"]), true).unwrap();
    }

    #[test]
    fn preflight_missing_nf_conntrack_typed() {
        let err =
            classify_nf_conntrack_availability(&HashSet::from(["tap", "bridge"]), false)
                .unwrap_err();

        assert!(matches!(err, PreflightError::NfConntrackUnavailable));
    }

    #[test]
    fn nf_conntrack_module_satisfies_nat_preflight() {
        classify_nf_conntrack_availability(
            &HashSet::from(["tap", "bridge", "nf_conntrack"]),
            false,
        )
        .unwrap();
    }

    #[test]
    fn nf_conntrack_sys_module_satisfies_nat_preflight() {
        classify_nf_conntrack_availability(&HashSet::from(["tap", "bridge"]), true).unwrap();
    }

    #[test]
    fn required_kernel_modules_report_missing_tap_bridge() {
        let err = classify_required_modules(&HashSet::from(["vhost_vsock"])).unwrap_err();

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
    fn non_access_kvm_open_errors_fail_closed() {
        // Non-EACCES open errors on /dev/kvm (EBUSY, EIO, etc.) must propagate
        // — silently passing the check on an unknown device state hides a
        // misconfiguration that will fail at boot anyway.
        let result = classify_kvm_access(
            PathBuf::from("/dev/kvm").as_path(),
            true,
            Err(io::Error::new(io::ErrorKind::Other, "busy")),
        );

        assert!(matches!(result, Err(PreflightError::PathIo { .. })));
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
