//! The ordered preflight checks that populate a [`Discovery`].

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::artifacts::{verify_artifacts, ArtifactPreflightConfig};
use crate::binary::{discover_binaries, verify_host_binaries, BinaryDiscoveryConfig};
use crate::cache::PreflightCache;
use crate::firecracker_train::enforce_firecracker_version;
use crate::{CheckRow, Discovery, PreflightError};

#[path = "substrate.rs"]
mod substrate;

#[cfg(test)]
pub(crate) use substrate::{
    check_cgroup_mode_with_probe, classify_cgroup_probe, classify_host_kernel_release,
    classify_jailer_identity, classify_kvm_access, parse_cgroup_mode, parse_jail_id,
};
pub use substrate::{
    verify_host_substrate, verify_host_substrate_fixture, CgroupPreflightMode,
    HostFeaturePreflightConfig, HostSubstrateDiscovery, HostSubstrateFixture,
    HostSubstrateFixtureKvm, HostSubstrateProofKind,
};

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
const BR_NETFILTER_MODULE_PATH: &str = "/sys/module/br_netfilter";
const BRIDGE_NF_CALL_IPTABLES_PATH: &str = "/proc/sys/net/bridge/bridge-nf-call-iptables";
const NF_CONNTRACK_MODULE_PATH: &str = "/sys/module/nf_conntrack";
const NF_CONNTRACK_MAX_PATH: &str = "/proc/sys/net/netfilter/nf_conntrack_max";
const THP_ENABLED_PATH: &str = "/sys/kernel/mm/transparent_hugepage/enabled";
const TUN_PATH: &str = "/dev/net/tun";
const VHOST_VSOCK_PATH: &str = "/dev/vhost-vsock";
const ENV_SKIP_CPU_VULNERABILITIES: &str = "M80_SKIP_CHECK_VULNERABILITIES";
const NF_CONNTRACK_ENTRIES_PER_VM: u64 = 1_000;
const NF_CONNTRACK_HEADROOM_MULTIPLIER: u64 = 2;

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
    let substrate = verify_host_substrate(host_feature_config)?;
    report.extend(substrate.report);

    // KVM CPU extensions
    check_kvm_cpu_extensions(&mut report)?;

    // Kernel modules
    check_kernel_modules(&mut report)?;

    // Host tuning advisories
    check_thp_policy(&mut report);
    check_kvm_halt_poll(&mut report);
    check_cpu_governor(&mut report);
    check_cpu_microcode(&mut report);
    check_cpu_vulnerabilities(&mut report)?;
    check_nf_conntrack_capacity(host_feature_config.expected_concurrent_vms, &mut report)?;

    let privilege = substrate.privilege;

    let cache = PreflightCache::load(&binary_config, &artifact_config);

    // Firecracker and jailer binaries
    let binaries = discover_binaries(
        &binary_config,
        cache.hit().map(|hit| hit.firecracker_version.as_str()),
        cache.hit().map(|hit| hit.jailer_version.as_str()),
    )?;
    let host_binary_manifest = artifact_config
        .artifact_dir
        .join("host-binaries.manifest.json");
    verify_host_binaries(&binary_config, &binaries, &host_binary_manifest)?;
    report.push(CheckRow {
        label: "Firecracker binary".to_string(),
        passed: true,
        detail: format!(
            "{} (observed {}, configured expected {})",
            binaries.firecracker_bin.display(),
            binaries.firecracker_version,
            binary_config
                .expected_firecracker_version
                .as_deref()
                .unwrap_or("from guest manifest")
        ),
    });
    report.push(CheckRow {
        label: "Firecracker seccomp filter".to_string(),
        passed: true,
        detail: binaries.firecracker_seccomp_filter.display().to_string(),
    });

    report.push(CheckRow {
        label: "Jailer binary".to_string(),
        passed: true,
        detail: format!(
            "{} (observed {}, expected {})",
            binaries.jailer_bin.display(),
            binaries.jailer_version,
            binaries.firecracker_version
        ),
    });
    report.push(CheckRow {
        label: "Jailer hardening wrapper".to_string(),
        passed: true,
        detail: binaries.jailer_harden_bin.display().to_string(),
    });
    report.push(CheckRow {
        label: "Network helper".to_string(),
        passed: true,
        detail: binaries.net_helper_bin.display().to_string(),
    });
    report.push(CheckRow {
        label: "Host binary manifest".to_string(),
        passed: true,
        detail: format!("{} (sha256 ok)", host_binary_manifest.display()),
    });

    // 14-18. Kernel/rootfs artifacts, run-root, and storage helpers
    let artifacts = verify_artifacts(&artifact_config, cache.hit().map(|hit| &hit.manifest))?;
    check_manifest_firecracker_train(
        &artifacts.manifest.expected_firecracker_version,
        &binaries.firecracker_version,
    )?;
    if let Some(row) = report
        .iter_mut()
        .find(|row| row.label == "Firecracker binary")
    {
        row.detail = format!(
            "{} (observed {}, expected {})",
            binaries.firecracker_bin.display(),
            binaries.firecracker_version,
            artifacts.manifest.expected_firecracker_version
        );
    }
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

    cache.store(
        &binaries.firecracker_version,
        &binaries.jailer_version,
        &artifacts.manifest,
    );

    Ok(Discovery {
        firecracker_bin: binaries.firecracker_bin,
        firecracker_seccomp_filter: binaries.firecracker_seccomp_filter,
        jailer_bin: binaries.jailer_bin,
        firecracker_version: binaries.firecracker_version,
        jailer_version: binaries.jailer_version,
        jailer_harden_bin: binaries.jailer_harden_bin,
        net_helper_bin: binaries.net_helper_bin,
        kernel: artifacts.kernel,
        rootfs: artifacts.rootfs,
        pinned_rootfs: artifacts.pinned_rootfs,
        manifest: artifacts.manifest,
        run_root: artifacts.run_root,
        privilege,
        report,
    })
}

fn check_manifest_firecracker_train(
    expected_firecracker_version: &str,
    actual_firecracker_version: &str,
) -> Result<(), PreflightError> {
    enforce_firecracker_version(expected_firecracker_version, actual_firecracker_version)
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
    classify_br_netfilter_availability(
        &loaded,
        std::path::Path::new(BR_NETFILTER_MODULE_PATH).exists(),
    )?;
    check_bridge_nf_call_iptables()?;
    classify_required_modules(&loaded)?;

    report.push(CheckRow {
        label: "Kernel modules".to_string(),
        passed: true,
        detail: "tap, bridge, br_netfilter loaded; tun, vhost-vsock, nf_conntrack available; bridge-nf-call-iptables=1".to_string(),
    });
    Ok(())
}

fn check_thp_policy(report: &mut Vec<CheckRow>) {
    report.push(classify_thp_policy(fs::read_to_string(THP_ENABLED_PATH)));
}

fn check_nf_conntrack_capacity(
    expected_concurrent_vms: u32,
    report: &mut Vec<CheckRow>,
) -> Result<(), PreflightError> {
    let raw =
        fs::read_to_string(NF_CONNTRACK_MAX_PATH).map_err(|source| PreflightError::PathIo {
            path: PathBuf::from(NF_CONNTRACK_MAX_PATH),
            source,
        })?;
    let actual = classify_nf_conntrack_capacity(&raw, expected_concurrent_vms)?;
    let minimum = minimum_nf_conntrack_entries(expected_concurrent_vms);
    report.push(CheckRow {
        label: "Conntrack capacity".to_string(),
        passed: true,
        detail: format!(
            "nf_conntrack_max={actual} >= {minimum} for {expected_concurrent_vms} expected concurrent VMs"
        ),
    });
    Ok(())
}

fn classify_nf_conntrack_capacity(
    raw: &str,
    expected_concurrent_vms: u32,
) -> Result<u64, PreflightError> {
    let actual = raw
        .trim()
        .parse::<u64>()
        .map_err(|_| PreflightError::InvalidNfConntrackMax {
            actual: raw.trim().to_owned(),
        })?;
    let minimum = minimum_nf_conntrack_entries(expected_concurrent_vms);
    if actual < minimum {
        return Err(PreflightError::NfConntrackCapacityTooLow {
            actual,
            minimum,
            expected_concurrent_vms,
        });
    }
    Ok(actual)
}

fn minimum_nf_conntrack_entries(expected_concurrent_vms: u32) -> u64 {
    u64::from(expected_concurrent_vms)
        * NF_CONNTRACK_ENTRIES_PER_VM
        * NF_CONNTRACK_HEADROOM_MULTIPLIER
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

fn classify_br_netfilter_availability(
    loaded_modules: &HashSet<&str>,
    sys_module_exists: bool,
) -> Result<(), PreflightError> {
    if loaded_modules.contains("br_netfilter") || sys_module_exists {
        return Ok(());
    }
    Err(PreflightError::BridgeNetfilterUnavailable)
}

fn check_bridge_nf_call_iptables() -> Result<(), PreflightError> {
    let raw = fs::read_to_string(BRIDGE_NF_CALL_IPTABLES_PATH).map_err(|source| {
        PreflightError::PathIo {
            path: PathBuf::from(BRIDGE_NF_CALL_IPTABLES_PATH),
            source,
        }
    })?;
    classify_bridge_nf_call_iptables(&raw)
}

fn classify_bridge_nf_call_iptables(raw: &str) -> Result<(), PreflightError> {
    let actual = raw.trim();
    if actual == "1" {
        return Ok(());
    }
    Err(PreflightError::BridgeNfCallIptablesDisabled {
        actual: actual.to_owned(),
    })
}

fn classify_required_modules(loaded: &HashSet<&str>) -> Result<(), PreflightError> {
    let missing: Vec<_> = REQUIRED_KERNEL_MODULES
        .iter()
        .filter(|m| !loaded.contains(*m))
        .map(ToString::to_string)
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(PreflightError::KernelModulesMissing { missing })
}

#[cfg(test)]
#[path = "checks_tests.rs"]
mod checks_tests;
