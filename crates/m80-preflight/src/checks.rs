//! The 10 ordered preflight checks that populate a [`Discovery`].

use std::fs;
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

/// Run all 10 checks in order. First failure returns a typed error immediately.
pub fn run() -> Result<Discovery, PreflightError> {
    let mut report: Vec<CheckRow> = Vec::new();

    // 1. OS gate
    check_os(&mut report)?;

    // 2. KVM
    check_kvm(&mut report)?;

    // 3. Kernel modules
    check_kernel_modules(&mut report)?;

    // 4. Privilege
    let privilege = check_privilege(&mut report)?;

    // 5-6. Firecracker and jailer binaries
    let binaries = discover_binaries(&BinaryDiscoveryConfig::from_env())?;
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

    // 7-10. Kernel/rootfs artifacts, run-root, and storage helpers
    let artifacts = verify_artifacts(&ArtifactPreflightConfig::from_env())?;
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
    let sysname = uts.sysname().to_string_lossy().to_string();
    if sysname != "Linux" {
        return Err(PreflightError::UnsupportedHostPlatform { actual: sysname });
    }
    let release = uts.release().to_string_lossy().to_string();
    report.push(CheckRow {
        label: "OS gate".to_string(),
        passed: true,
        detail: format!("Linux {release}"),
    });
    Ok(())
}

fn check_kvm(report: &mut Vec<CheckRow>) -> Result<(), PreflightError> {
    let kvm = PathBuf::from(KVM_PATH);

    if !kvm.exists() {
        return Err(PreflightError::KvmUnavailable { path: kvm });
    }

    // Write-access check: open O_WRONLY; close immediately.
    // EACCES → permission denied → fail; other errors (EBUSY etc.) are not
    // access-denial and we don't block on them.
    let result = fs::OpenOptions::new().write(true).open(&kvm);

    match result {
        Ok(f) => drop(f),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            return Err(PreflightError::KvmNotWritable { path: kvm });
        }
        Err(_) => {}
    }

    report.push(CheckRow {
        label: "KVM".to_string(),
        passed: true,
        detail: format!("{} present and writable", kvm.display()),
    });
    Ok(())
}

fn check_kernel_modules(report: &mut Vec<CheckRow>) -> Result<(), PreflightError> {
    let content = fs::read_to_string("/proc/modules").map_err(PreflightError::Io)?;
    let loaded: Vec<&str> = content
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect();

    let required = &["tap", "bridge"];
    let mut missing: Vec<String> = Vec::new();
    for module in required {
        if !loaded.iter().any(|&m| m == *module) {
            missing.push(module.to_string());
        }
    }

    if !missing.is_empty() {
        return Err(PreflightError::KernelModulesMissing { missing });
    }

    report.push(CheckRow {
        label: "Kernel modules".to_string(),
        passed: true,
        detail: "tap, bridge loaded".to_string(),
    });
    Ok(())
}

fn check_privilege(report: &mut Vec<CheckRow>) -> Result<PrivilegeStatus, PreflightError> {
    let euid = geteuid().as_raw();
    let effective = caps::read(None, CapSet::Effective)
        .map_err(|e| PreflightError::Io(std::io::Error::other(e.to_string())))?;
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
