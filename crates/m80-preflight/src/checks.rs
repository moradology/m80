//! The 10 ordered preflight checks that populate a [`Discovery`].

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use caps::CapSet;
use nix::sys::statvfs::statvfs;
use nix::sys::utsname::uname;
use nix::unistd::geteuid;

use m80_image_manifest::Manifest;

use crate::{
    CheckRow, Discovery, PreflightError, PrivilegeStatus, REQUIRED_CAPABILITIES,
};

// ── Env-var knobs ────────────────────────────────────────────────────────────

const ENV_FIRECRACKER_BIN: &str = "M80_FIRECRACKER_BIN";
const ENV_FIRECRACKER_VERSION: &str = "M80_FIRECRACKER_VERSION";
const ENV_JAILER_BIN: &str = "M80_JAILER_BIN";
const ENV_KERNEL_IMAGE: &str = "M80_KERNEL_IMAGE";
const ENV_ROOTFS_IMAGE: &str = "M80_ROOTFS_IMAGE";
const ENV_ARTIFACT_DIR: &str = "M80_ARTIFACT_DIR";
const ENV_RUN_ROOT: &str = "M80_RUN_ROOT";

const DEFAULT_FIRECRACKER_BIN: &str = "/opt/firecracker/bin/firecracker";
const DEFAULT_JAILER_BIN: &str = "/opt/firecracker/bin/jailer";
const DEFAULT_ARTIFACT_DIR: &str = "/opt/m80/artifacts";
const DEFAULT_RUN_ROOT: &str = "/var/run/m80";

/// 100 MiB minimum free space for the run-root.
const MIN_RUN_ROOT_FREE_BYTES: u64 = 100 * 1024 * 1024;

const REQUIRED_STORAGE_HELPERS: &[&str] = &["mkfs.ext4", "debugfs", "e2fsck"];

// ── Entry point ──────────────────────────────────────────────────────────────

/// Run all 10 checks in order. First failure returns a typed error immediately.
pub(crate) fn run_all() -> Result<Discovery, PreflightError> {
    let mut report: Vec<CheckRow> = Vec::new();

    // 1. OS gate
    let sysname = check_os(&mut report)?;
    let _ = sysname; // consumed by the check; detail is in the row

    // 2. KVM
    check_kvm(&mut report)?;

    // 3. Kernel modules
    check_kernel_modules(&mut report)?;

    // 4. Privilege
    let privilege = check_privilege(&mut report)?;

    // 5. Firecracker binary
    let firecracker_bin = check_firecracker_bin(&mut report)?;

    // 6. Jailer binary
    let jailer_bin = check_jailer_bin(&mut report)?;

    // 7. Kernel artifact
    let kernel = check_kernel(&mut report)?;

    // 8. Rootfs + manifest
    let (rootfs, manifest) = check_rootfs_and_manifest(&mut report)?;

    // 9. Run-root
    let run_root = check_run_root(&mut report)?;

    // 10. Storage helpers
    check_storage_helpers(&mut report)?;

    Ok(Discovery {
        firecracker_bin,
        jailer_bin,
        kernel,
        rootfs,
        manifest,
        run_root,
        privilege,
        report,
    })
}

// ── Individual checks ────────────────────────────────────────────────────────

fn check_os(report: &mut Vec<CheckRow>) -> Result<String, PreflightError> {
    let uts = uname().map_err(|e| PreflightError::Io(e.into()))?;
    let sysname = uts.sysname().to_string_lossy().to_string();
    if sysname != "Linux" {
        return Err(PreflightError::UnsupportedHostPlatform(sysname));
    }
    let release = uts.release().to_string_lossy().to_string();
    report.push(CheckRow {
        label: "OS gate".to_string(),
        passed: true,
        detail: format!("Linux {release}"),
    });
    Ok(sysname)
}

fn check_kvm(report: &mut Vec<CheckRow>) -> Result<(), PreflightError> {
    let kvm = Path::new("/dev/kvm");

    // Existence check.
    if fs::metadata(kvm).is_err() {
        return Err(PreflightError::KvmUnavailable);
    }

    // Write-access check: open O_WRONLY; close immediately.
    // EACCES → permission denied → fail; other errors (EBUSY etc.) are not
    // access-denial and we don't block on them.
    let result = fs::OpenOptions::new().write(true).open(kvm);

    match result {
        Ok(f) => drop(f),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            return Err(PreflightError::KvmUnavailable);
        }
        Err(_) => {}
    }

    report.push(CheckRow {
        label: "KVM".to_string(),
        passed: true,
        detail: "/dev/kvm present and writable".to_string(),
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
    if geteuid().as_raw() == 0 {
        report.push(CheckRow {
            label: "Privilege".to_string(),
            passed: true,
            detail: "euid == 0 (root)".to_string(),
        });
        return Ok(PrivilegeStatus::Root);
    }

    let effective = caps::read(None, CapSet::Effective)
        .map_err(|e| PreflightError::Io(std::io::Error::other(e.to_string())))?;

    let missing_caps: Vec<_> = REQUIRED_CAPABILITIES
        .iter()
        .filter(|cap| !effective.contains(cap))
        .copied()
        .collect();

    if !missing_caps.is_empty() {
        return Err(PreflightError::PrivilegeUnavailable { missing_caps });
    }

    report.push(CheckRow {
        label: "Privilege".to_string(),
        passed: true,
        detail: format!(
            "capability-bearing ({})",
            REQUIRED_CAPABILITIES
                .iter()
                .map(|c| format!("{c:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    });
    Ok(PrivilegeStatus::CapabilityBearing)
}

fn check_firecracker_bin(report: &mut Vec<CheckRow>) -> Result<PathBuf, PreflightError> {
    let bin = env::var_os(ENV_FIRECRACKER_BIN)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_FIRECRACKER_BIN));

    if fs::metadata(&bin).is_err() {
        return Err(PreflightError::FirecrackerBinaryNotFound);
    }

    // Run --version and parse first line.
    let out = Command::new(&bin)
        .arg("--version")
        .output()
        .map_err(|_| PreflightError::FirecrackerBinaryNotFound)?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    let first_line = stdout.lines().next().unwrap_or("").trim();
    // Typical output: "Firecracker v1.15.1"
    let actual_version = first_line
        .split_whitespace()
        .last()
        .unwrap_or(first_line)
        .to_string();

    if let Ok(expected) = env::var(ENV_FIRECRACKER_VERSION) {
        if actual_version != expected {
            return Err(PreflightError::FirecrackerVersionMismatch {
                expected,
                actual: actual_version,
            });
        }
    }

    report.push(CheckRow {
        label: "Firecracker binary".to_string(),
        passed: true,
        detail: format!("{} ({})", bin.display(), actual_version),
    });
    Ok(bin)
}

fn check_jailer_bin(report: &mut Vec<CheckRow>) -> Result<PathBuf, PreflightError> {
    let bin = env::var_os(ENV_JAILER_BIN)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_JAILER_BIN));

    if fs::metadata(&bin).is_err() {
        return Err(PreflightError::JailerBinaryNotFound);
    }

    report.push(CheckRow {
        label: "Jailer binary".to_string(),
        passed: true,
        detail: bin.display().to_string(),
    });
    Ok(bin)
}

fn check_kernel(report: &mut Vec<CheckRow>) -> Result<PathBuf, PreflightError> {
    if let Some(path) = env::var_os(ENV_KERNEL_IMAGE).map(PathBuf::from) {
        if fs::metadata(&path).is_err() {
            return Err(PreflightError::KernelNotFound);
        }
        report.push(CheckRow {
            label: "Kernel image".to_string(),
            passed: true,
            detail: path.display().to_string(),
        });
        return Ok(path);
    }

    let artifact_dir = env::var_os(ENV_ARTIFACT_DIR)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_ARTIFACT_DIR));

    let kernel = discover_vmlinux(&artifact_dir)?;

    report.push(CheckRow {
        label: "Kernel image".to_string(),
        passed: true,
        detail: kernel.display().to_string(),
    });
    Ok(kernel)
}

/// Return the lexicographically-largest `vmlinux-*` file under `dir`.
fn discover_vmlinux(dir: &Path) -> Result<PathBuf, PreflightError> {
    let entries = fs::read_dir(dir).map_err(PreflightError::Io)?;

    let mut candidates: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(PreflightError::Io)?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with("vmlinux-") {
            candidates.push(entry.path());
        }
    }

    candidates.sort();
    candidates.pop().ok_or(PreflightError::KernelNotFound)
}

fn check_rootfs_and_manifest(
    report: &mut Vec<CheckRow>,
) -> Result<(PathBuf, Manifest), PreflightError> {
    let rootfs = env::var_os(ENV_ROOTFS_IMAGE)
        .map(PathBuf::from)
        .ok_or(PreflightError::RootfsNotFound)?;

    if fs::metadata(&rootfs).is_err() {
        return Err(PreflightError::RootfsNotFound);
    }

    let manifest_path = PathBuf::from(format!("{}.manifest.json", rootfs.display()));
    let manifest = Manifest::read(&manifest_path)?;

    let parent = rootfs
        .parent()
        .unwrap_or_else(|| Path::new("/"))
        .to_path_buf();
    manifest.verify(&parent)?;

    report.push(CheckRow {
        label: "Rootfs + manifest".to_string(),
        passed: true,
        detail: format!("{} (sha256 ok)", rootfs.display()),
    });
    Ok((rootfs, manifest))
}

fn check_run_root(report: &mut Vec<CheckRow>) -> Result<PathBuf, PreflightError> {
    let run_root = env::var_os(ENV_RUN_ROOT)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_RUN_ROOT));

    if !run_root.is_absolute() {
        return Err(PreflightError::RunRootUnavailable {
            reason: format!("path is not absolute: {}", run_root.display()),
        });
    }

    // Must already exist — no silent creation per CLAUDE.md.
    if fs::metadata(&run_root).is_err() {
        return Err(PreflightError::RunRootUnavailable {
            reason: format!("directory does not exist: {}", run_root.display()),
        });
    }

    // Free-space check via statvfs.
    let stat = statvfs(&run_root).map_err(|e| PreflightError::RunRootUnavailable {
        reason: format!("statvfs failed on {}: {e}", run_root.display()),
    })?;

    let free_bytes = stat.blocks_available() as u64 * stat.fragment_size() as u64;
    if free_bytes < MIN_RUN_ROOT_FREE_BYTES {
        return Err(PreflightError::RunRootUnavailable {
            reason: format!(
                "{} has only {} MiB free (need at least 100 MiB)",
                run_root.display(),
                free_bytes / (1024 * 1024),
            ),
        });
    }

    report.push(CheckRow {
        label: "Run-root".to_string(),
        passed: true,
        detail: format!(
            "{} ({} MiB free)",
            run_root.display(),
            free_bytes / (1024 * 1024),
        ),
    });
    Ok(run_root)
}

fn check_storage_helpers(report: &mut Vec<CheckRow>) -> Result<(), PreflightError> {
    let search_path = env::var_os("PATH");
    for helper in REQUIRED_STORAGE_HELPERS {
        if find_in_path(helper, search_path.as_ref()).is_none() {
            return Err(PreflightError::StorageHelperMissing(helper.to_string()));
        }
    }
    report.push(CheckRow {
        label: "Storage helpers".to_string(),
        passed: true,
        detail: REQUIRED_STORAGE_HELPERS.join(", "),
    });
    Ok(())
}

/// Look up `binary` in `search_path` (a `PATH`-style colon-separated list).
fn find_in_path(binary: &str, search_path: Option<&std::ffi::OsString>) -> Option<PathBuf> {
    let search_path = search_path?;
    env::split_paths(search_path)
        .map(|dir| dir.join(binary))
        .find(|candidate| candidate.exists())
}
