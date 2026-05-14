//! The ordered preflight checks that populate a [`Discovery`].

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use caps::CapSet;
use nix::sys::utsname::uname;
use nix::unistd::{geteuid, Gid, Group, Uid, User};

use crate::artifacts::{verify_artifacts, ArtifactPreflightConfig};
use crate::binary::{discover_binaries, BinaryDiscoveryConfig};
use crate::cache::PreflightCache;
use crate::{
    classify_privilege, CheckRow, Discovery, PreflightError, PrivilegeStatus, REQUIRED_CAPABILITIES,
};

const KVM_PATH: &str = "/dev/kvm";
const KVM_HALT_POLL_NS_PATH: &str = "/sys/module/kvm/parameters/halt_poll_ns";
const KVM_HALT_POLL_NS_GROW_PATH: &str = "/sys/module/kvm/parameters/halt_poll_ns_grow";
const KVM_HALT_POLL_NS_SHRINK_PATH: &str = "/sys/module/kvm/parameters/halt_poll_ns_shrink";
const KVM_LAPIC_TIMER_ADVANCE_PATH: &str = "/sys/module/kvm/parameters/lapic_timer_advance";
const KVM_INTEL_PREEMPTION_TIMER_PATH: &str =
    "/sys/module/kvm_intel/parameters/enable_preemption_timer";
const CPUFREQ_SCALING_DRIVER_PATH: &str = "/sys/devices/system/cpu/cpu0/cpufreq/scaling_driver";
const CPUFREQ_SCALING_GOVERNOR_PATH: &str = "/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor";
const CPU_MICROCODE_VERSION_PATH: &str = "/sys/devices/system/cpu/cpu0/microcode/version";
const CPU_MICROCODE_FLAGS_PATH: &str = "/sys/devices/system/cpu/cpu0/microcode/processor_flags";
const CPU_VULNERABILITY_DIR: &str = "/sys/devices/system/cpu/vulnerabilities";
const NF_CONNTRACK_MODULE_PATH: &str = "/sys/module/nf_conntrack";
const THP_ENABLED_PATH: &str = "/sys/kernel/mm/transparent_hugepage/enabled";
const TUN_PATH: &str = "/dev/net/tun";
const VHOST_VSOCK_PATH: &str = "/dev/vhost-vsock";
const ENV_SKIP_CPU_VULNERABILITIES: &str = "M80_SKIP_CHECK_VULNERABILITIES";
const ENV_JAIL_UID: &str = "M80_JAIL_UID";
const ENV_JAIL_GID: &str = "M80_JAIL_GID";
const DEFAULT_JAIL_UID: u32 = 3000;
const DEFAULT_JAIL_GID: u32 = 3000;
const MIN_HOST_KERNEL_MAJOR: u64 = 6;
const MIN_HOST_KERNEL_MINOR: u64 = 1;

const REQUIRED_KERNEL_MODULES: &[&str] = &["tap", "bridge"];
const CPU_VULNERABILITY_CHECKS: &[CpuVulnerabilityCheck] = &[
    CpuVulnerabilityCheck {
        id: "mds",
        hard_fail_on_vulnerable: true,
    },
    CpuVulnerabilityCheck {
        id: "l1tf",
        hard_fail_on_vulnerable: true,
    },
    CpuVulnerabilityCheck {
        id: "spectre_v2",
        hard_fail_on_vulnerable: false,
    },
    CpuVulnerabilityCheck {
        id: "retbleed",
        hard_fail_on_vulnerable: false,
    },
    CpuVulnerabilityCheck {
        id: "tsx_async_abort",
        hard_fail_on_vulnerable: false,
    },
    CpuVulnerabilityCheck {
        id: "srbds",
        hard_fail_on_vulnerable: false,
    },
    CpuVulnerabilityCheck {
        id: "mmio_stale_data",
        hard_fail_on_vulnerable: false,
    },
    CpuVulnerabilityCheck {
        id: "gather_data_sampling",
        hard_fail_on_vulnerable: false,
    },
];

#[derive(Debug, Clone, Copy)]
struct CpuVulnerabilityCheck {
    id: &'static str,
    hard_fail_on_vulnerable: bool,
}

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
    /// UID that the official jailer will switch Firecracker to.
    pub jail_uid: u32,
    /// GID that the official jailer will switch Firecracker to.
    pub jail_gid: u32,
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
        let jail_uid = parse_jail_id_env(ENV_JAIL_UID, DEFAULT_JAIL_UID)?;
        let jail_gid = parse_jail_id_env(ENV_JAIL_GID, DEFAULT_JAIL_GID)?;
        Ok(Self {
            cgroup_mode,
            jail_uid,
            jail_gid,
        })
    }
}

fn parse_jail_id_env(env_key: &'static str, default: u32) -> Result<u32, PreflightError> {
    match std::env::var(env_key) {
        Ok(value) => parse_jail_id(env_key, &value),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(std::env::VarError::NotUnicode(value)) => Err(PreflightError::InvalidJailIdentity {
            field: env_key,
            value: value.to_string_lossy().into_owned(),
        }),
    }
}

fn parse_jail_id(field: &'static str, value: &str) -> Result<u32, PreflightError> {
    value
        .parse()
        .map_err(|_| PreflightError::InvalidJailIdentity {
            field,
            value: value.to_owned(),
        })
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

    // 2. Host kernel floor
    check_host_kernel_floor(&mut report)?;

    // 3. KVM
    check_kvm(&mut report)?;

    // 4. KVM CPU extensions
    check_kvm_cpu_extensions(&mut report)?;

    // 5. Kernel modules
    check_kernel_modules(&mut report)?;

    // 6. Host tuning advisories
    check_thp_policy(&mut report);
    check_kvm_halt_poll(&mut report);
    check_cpu_governor(&mut report);
    check_cpu_microcode(&mut report);
    check_cpu_vulnerabilities(&mut report)?;

    // 11. Cgroup host mode
    check_cgroup_mode(host_feature_config.cgroup_mode, &mut report)?;

    // 12. Jailer identity
    check_jailer_identity(
        host_feature_config.jail_uid,
        host_feature_config.jail_gid,
        &mut report,
    )?;

    // 13. Privilege
    let privilege = check_privilege(&mut report)?;

    let cache = PreflightCache::load(&binary_config, &artifact_config);

    // 11-13. Firecracker and jailer binaries
    let binaries = discover_binaries(
        &binary_config,
        cache.hit().map(|hit| hit.firecracker_version.as_str()),
    )?;
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

    // 14-18. Kernel/rootfs artifacts, run-root, and storage helpers
    let artifacts = verify_artifacts(&artifact_config, cache.hit().map(|hit| &hit.manifest))?;
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
        label: "Run-root filesystem".to_string(),
        passed: true,
        detail: artifacts.run_root_reflink.detail(),
    });

    report.push(CheckRow {
        label: "Storage helpers".to_string(),
        passed: true,
        detail: artifacts.storage_helpers.join(", "),
    });

    cache.store(&binaries.firecracker_version, &artifacts.manifest);

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

fn check_host_kernel_floor(report: &mut Vec<CheckRow>) -> Result<(), PreflightError> {
    let uts = uname().map_err(|e| PreflightError::SystemIo {
        operation: "uname",
        source: e.into(),
    })?;
    let release = uts.release().to_string_lossy().into_owned();
    classify_host_kernel_release(&release)?;
    report.push(CheckRow {
        label: "Host kernel floor".to_string(),
        passed: true,
        detail: format!("Linux {release} >= 6.1"),
    });
    Ok(())
}

fn classify_host_kernel_release(release: &str) -> Result<(), PreflightError> {
    let (major, minor) =
        parse_kernel_major_minor(release).ok_or_else(|| PreflightError::HostKernelUnsupported {
            actual: release.to_owned(),
            minimum: minimum_host_kernel_string(),
        })?;
    if (major, minor) < (MIN_HOST_KERNEL_MAJOR, MIN_HOST_KERNEL_MINOR) {
        return Err(PreflightError::HostKernelUnsupported {
            actual: release.to_owned(),
            minimum: minimum_host_kernel_string(),
        });
    }
    Ok(())
}

fn parse_kernel_major_minor(release: &str) -> Option<(u64, u64)> {
    let mut parts = release.split(['.', '-']);
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

fn minimum_host_kernel_string() -> String {
    format!("{MIN_HOST_KERNEL_MAJOR}.{MIN_HOST_KERNEL_MINOR}")
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

fn check_thp_policy(report: &mut Vec<CheckRow>) {
    report.push(classify_thp_policy(fs::read_to_string(THP_ENABLED_PATH)));
}

fn classify_thp_policy(read_result: Result<String, io::Error>) -> CheckRow {
    let detail = match read_result {
        Ok(raw) => match selected_thp_mode(&raw) {
            Some("always") => {
                "always selected; THP may back Firecracker guest memory without explicit hugepage reservation".to_string()
            }
            Some("madvise") => {
                "madvise selected; advisory: Firecracker guest memory will not get THP unless explicitly madvised; consider always for latency hosts (docs/ops/host-tuning.md)".to_string()
            }
            Some("never") => {
                "never selected; advisory: THP is disabled for Firecracker guest memory; consider always for latency hosts (docs/ops/host-tuning.md)".to_string()
            }
            Some(mode) => format!(
                "{mode} selected; advisory: unrecognized THP policy value at {THP_ENABLED_PATH}"
            ),
            None => format!("unrecognized THP policy format at {THP_ENABLED_PATH}: {raw:?}"),
        },
        Err(err) => format!(
            "unavailable at {THP_ENABLED_PATH}: {err}; advisory could not be evaluated (docs/ops/host-tuning.md)"
        ),
    };

    CheckRow {
        label: "Transparent hugepages".to_string(),
        passed: true,
        detail,
    }
}

fn selected_thp_mode(raw: &str) -> Option<&str> {
    raw.split_whitespace().find_map(|token| {
        token
            .strip_prefix('[')
            .and_then(|inner| inner.strip_suffix(']'))
    })
}

fn check_kvm_halt_poll(report: &mut Vec<CheckRow>) {
    report.push(classify_kvm_halt_poll(
        read_trimmed_sysfs(KVM_HALT_POLL_NS_PATH).as_deref(),
        read_trimmed_sysfs(KVM_HALT_POLL_NS_GROW_PATH).as_deref(),
        read_trimmed_sysfs(KVM_HALT_POLL_NS_SHRINK_PATH).as_deref(),
        read_trimmed_sysfs(KVM_LAPIC_TIMER_ADVANCE_PATH).as_deref(),
        read_trimmed_sysfs(KVM_INTEL_PREEMPTION_TIMER_PATH).as_deref(),
    ));
}

fn classify_kvm_halt_poll(
    halt_poll_ns: Option<&str>,
    grow: Option<&str>,
    shrink: Option<&str>,
    lapic_timer_advance: Option<&str>,
    intel_preemption_timer: Option<&str>,
) -> CheckRow {
    let mut parts = Vec::new();
    match halt_poll_ns {
        Some(value) => parts.push(format!("halt_poll_ns={value}")),
        None => parts.push("halt_poll_ns=unavailable".to_string()),
    }
    if let Some(value) = grow {
        parts.push(format!("grow={value}"));
    }
    if let Some(value) = shrink {
        parts.push(format!("shrink={value}"));
    }
    if let Some(value) = lapic_timer_advance {
        parts.push(format!("lapic_timer_advance={value}"));
    }
    if let Some(value) = intel_preemption_timer {
        parts.push(format!("enable_preemption_timer={value}"));
    }

    let detail = if halt_poll_ns.is_some() {
        format!(
            "{}; advisory: latency-priority hosts may evaluate halt_poll_ns=400000; density-priority hosts may keep or lower the default (docs/ops/host-tuning.md)",
            parts.join(", ")
        )
    } else {
        format!(
            "{}; advisory could not be evaluated (docs/ops/host-tuning.md)",
            parts.join(", ")
        )
    };

    CheckRow {
        label: "KVM halt polling".to_string(),
        passed: true,
        detail,
    }
}

fn check_cpu_governor(report: &mut Vec<CheckRow>) {
    let driver = read_trimmed_sysfs(CPUFREQ_SCALING_DRIVER_PATH);
    let governor = read_trimmed_sysfs(CPUFREQ_SCALING_GOVERNOR_PATH);
    report.push(classify_cpu_governor(
        driver.as_deref(),
        governor.as_deref(),
    ));
}

fn classify_cpu_governor(driver: Option<&str>, governor: Option<&str>) -> CheckRow {
    let detail = match (driver, governor) {
        (Some(driver @ ("intel_pstate" | "amd_pstate")), Some(governor)) => format!(
            "driver={driver}, governor={governor}; hardware-managed pstate handles ramp; no m80 change recommended"
        ),
        (Some("acpi-cpufreq"), Some("performance")) => {
            "driver=acpi-cpufreq, governor=performance; software governor already performance"
                .to_string()
        }
        (Some("acpi-cpufreq"), Some(governor)) => format!(
            "driver=acpi-cpufreq, governor={governor}; advisory: consider `cpupower frequency-set -g performance` for tighter launch tail (docs/ops/host-tuning.md)"
        ),
        (Some(driver), Some(governor)) => format!(
            "driver={driver}, governor={governor}; unclassified cpufreq driver; no m80 change recommended"
        ),
        (Some(driver), None) => format!(
            "driver={driver}, governor=unavailable; CPU governor check not evaluated (docs/ops/host-tuning.md)"
        ),
        (None, Some(governor)) => format!(
            "driver=unavailable, governor={governor}; CPU governor check not evaluated (docs/ops/host-tuning.md)"
        ),
        (None, None) => {
            "driver=unavailable, governor=unavailable; CPU governor check not evaluated (docs/ops/host-tuning.md)".to_string()
        }
    };

    CheckRow {
        label: "CPU governor".to_string(),
        passed: true,
        detail,
    }
}

fn check_cpu_microcode(report: &mut Vec<CheckRow>) {
    report.push(classify_cpu_microcode(
        read_trimmed_sysfs(CPU_MICROCODE_VERSION_PATH).as_deref(),
        read_trimmed_sysfs(CPU_MICROCODE_FLAGS_PATH).as_deref(),
    ));
}

fn classify_cpu_microcode(version: Option<&str>, flags: Option<&str>) -> CheckRow {
    let detail = match (version, flags) {
        (Some(version), Some(flags)) => format!("version={version}, processor_flags={flags}"),
        (Some(version), None) => format!("version={version}, processor_flags=unavailable"),
        (None, Some(flags)) => format!("version=unavailable, processor_flags={flags}"),
        (None, None) => {
            "version=unavailable, processor_flags=unavailable; microcode level not reported by host"
                .to_string()
        }
    };
    CheckRow {
        label: "CPU microcode".to_string(),
        passed: true,
        detail,
    }
}

fn check_cpu_vulnerabilities(report: &mut Vec<CheckRow>) -> Result<(), PreflightError> {
    check_cpu_vulnerabilities_in_dir(
        Path::new(CPU_VULNERABILITY_DIR),
        cpu_vulnerability_skip_enabled(),
        report,
    )
}

fn cpu_vulnerability_skip_enabled() -> bool {
    std::env::var(ENV_SKIP_CPU_VULNERABILITIES).as_deref() == Ok("1")
}

fn check_cpu_vulnerabilities_in_dir(
    root: &Path,
    skip: bool,
    report: &mut Vec<CheckRow>,
) -> Result<(), PreflightError> {
    if skip {
        report.push(CheckRow {
            label: "CPU vulnerabilities".to_string(),
            passed: true,
            detail: format!("skipped by {ENV_SKIP_CPU_VULNERABILITIES}=1"),
        });
        return Ok(());
    }

    let mut observations = Vec::new();
    for check in CPU_VULNERABILITY_CHECKS {
        let path = root.join(check.id);
        observations.push(match fs::read_to_string(&path) {
            Ok(raw) => classify_cpu_vulnerability(*check, &raw)?,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                format!("{}=unavailable", check.id)
            }
            Err(err) => format!("{}=unreadable: {err}", check.id),
        });
    }

    report.push(CheckRow {
        label: "CPU vulnerabilities".to_string(),
        passed: true,
        detail: observations.join("; "),
    });
    Ok(())
}

fn classify_cpu_vulnerability(
    check: CpuVulnerabilityCheck,
    raw: &str,
) -> Result<String, PreflightError> {
    let detail = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if detail.starts_with("Vulnerable") {
        if check.hard_fail_on_vulnerable {
            return Err(PreflightError::CpuVulnerabilityDetected {
                id: check.id.to_owned(),
                detail,
            });
        }
        return Ok(format!("{}=vulnerable advisory: {detail}", check.id));
    }

    if detail.starts_with("Mitigation") || detail.starts_with("Not affected") {
        return Ok(format!("{}={detail}", check.id));
    }

    if detail.is_empty() {
        return Ok(format!("{}=unclassified advisory: empty status", check.id));
    }
    Ok(format!("{}=unclassified advisory: {detail}", check.id))
}

fn read_trimmed_sysfs(path: &str) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
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

fn check_jailer_identity(
    jail_uid: u32,
    jail_gid: u32,
    report: &mut Vec<CheckRow>,
) -> Result<(), PreflightError> {
    let user = User::from_uid(Uid::from_raw(jail_uid))
        .map_err(|source| PreflightError::SystemIo {
            operation: "user lookup",
            source: source.into(),
        })?
        .map(|user| user.name);
    let group = Group::from_gid(Gid::from_raw(jail_gid))
        .map_err(|source| PreflightError::SystemIo {
            operation: "group lookup",
            source: source.into(),
        })?
        .map(|group| group.name);

    report.push(classify_jailer_identity(jail_uid, jail_gid, user, group)?);
    Ok(())
}

fn classify_jailer_identity(
    jail_uid: u32,
    jail_gid: u32,
    user: Option<String>,
    group: Option<String>,
) -> Result<CheckRow, PreflightError> {
    let user = user.ok_or(PreflightError::JailIdentityUnavailable {
        field: "jail_uid",
        id: jail_uid,
    })?;
    let group = group.ok_or(PreflightError::JailIdentityUnavailable {
        field: "jail_gid",
        id: jail_gid,
    })?;

    Ok(CheckRow {
        label: "Jailer identity".to_string(),
        passed: true,
        detail: format!("uid={jail_uid} ({user}), gid={jail_gid} ({group})"),
    })
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
#[path = "checks_tests.rs"]
mod checks_tests;
