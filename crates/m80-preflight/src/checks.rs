//! The 10 ordered preflight checks that populate a [`Discovery`].

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use caps::CapSet;
use nix::sys::statvfs::statvfs;
use nix::sys::utsname::uname;
use nix::unistd::geteuid;

use m80_image_manifest::{KernelKind, Manifest};

use crate::{CheckRow, Discovery, PreflightError, PrivilegeStatus, REQUIRED_CAPABILITIES};

const ENV_FIRECRACKER_BIN: &str = "M80_FIRECRACKER_BIN";
const ENV_FIRECRACKER_VERSION: &str = "M80_FIRECRACKER_VERSION";
const ENV_JAILER_BIN: &str = "M80_JAILER_BIN";
const ENV_KERNEL_IMAGE: &str = "M80_KERNEL_IMAGE";
const ENV_KERNEL_KIND: &str = "M80_KERNEL_KIND";
const ENV_ROOTFS_IMAGE: &str = "M80_ROOTFS_IMAGE";
const ENV_ARTIFACT_DIR: &str = "M80_ARTIFACT_DIR";
const ENV_RUN_ROOT: &str = "M80_RUN_ROOT";

const KVM_PATH: &str = "/dev/kvm";
const DEFAULT_FIRECRACKER_BIN: &str = "/opt/firecracker/bin/firecracker";
const DEFAULT_JAILER_BIN: &str = "/opt/firecracker/bin/jailer";
const DEFAULT_ARTIFACT_DIR: &str = "/opt/m80/artifacts";
const DEFAULT_RUN_ROOT: &str = "/var/run/m80";

/// 100 MiB minimum free space for the run-root.
const MIN_RUN_ROOT_FREE_BYTES: u64 = 100 * 1024 * 1024;

const REQUIRED_STORAGE_HELPERS: &[&str] = &["mkfs.ext4", "cp", "fallocate", "debugfs", "e2fsck"];

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

    if !bin.exists() {
        return Err(PreflightError::FirecrackerBinaryNotFound);
    }

    // Run --version and parse first line. We just verified the file
    // exists, so a spawn failure here is something else (ENOEXEC,
    // permission denied, …) and the underlying io::Error is the useful
    // signal — surface it instead of remapping to "binary not found".
    let out = Command::new(&bin)
        .arg("--version")
        .output()
        .map_err(PreflightError::Io)?;

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

    if !bin.exists() {
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
        if !path.exists() {
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

    // Lexicographically-largest `vmlinux-*` under `artifact_dir`.
    let mut candidates: Vec<PathBuf> = fs::read_dir(&artifact_dir)
        .map_err(PreflightError::Io)?
        .filter_map(|entry| entry.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("vmlinux-"))
        .map(|e| e.path())
        .collect();
    candidates.sort();
    let kernel = candidates.pop().ok_or(PreflightError::KernelNotFound)?;

    report.push(CheckRow {
        label: "Kernel image".to_string(),
        passed: true,
        detail: kernel.display().to_string(),
    });
    Ok(kernel)
}

fn check_rootfs_and_manifest(
    report: &mut Vec<CheckRow>,
) -> Result<(PathBuf, Manifest), PreflightError> {
    let rootfs = env::var_os(ENV_ROOTFS_IMAGE)
        .map(PathBuf::from)
        .ok_or(PreflightError::RootfsNotFound)?;

    if !rootfs.exists() {
        return Err(PreflightError::RootfsNotFound);
    }

    let manifest_path = PathBuf::from(format!("{}.manifest.json", rootfs.display()));
    let mut manifest = Manifest::read(&manifest_path)?;
    if let Ok(kind) = env::var(ENV_KERNEL_KIND) {
        manifest.kernel_kind = parse_kernel_kind(&kind)?;
    }

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

fn parse_kernel_kind(raw: &str) -> Result<KernelKind, PreflightError> {
    match raw {
        "stock" => Ok(KernelKind::Stock),
        "stripped" => Ok(KernelKind::Stripped),
        other => Err(PreflightError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("M80_KERNEL_KIND must be stock|stripped, got {other}"),
        ))),
    }
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
    if !run_root.exists() {
        return Err(PreflightError::RunRootUnavailable {
            reason: format!("directory does not exist: {}", run_root.display()),
        });
    }

    // Free-space + mount-flags check via statvfs.
    let stat = statvfs(&run_root).map_err(|e| PreflightError::RunRootUnavailable {
        reason: format!("statvfs failed on {}: {e}", run_root.display()),
    })?;

    // `nodev` mounts forbid opening device nodes regardless of file
    // permissions. The jailer mknods /dev/kvm inside the per-VM chroot;
    // if the chroot lives on a `nodev` filesystem (default for /tmp on
    // many distros), firecracker fails InstanceStart with EACCES.
    if stat.flags().contains(nix::sys::statvfs::FsFlags::ST_NODEV) {
        return Err(PreflightError::RunRootUnavailable {
            reason: format!(
                "{} is on a `nodev` mount; device nodes (e.g. /dev/kvm) \
                 in the per-VM chroot will be unopenable. \
                 Pick a path on a filesystem that allows device nodes \
                 (e.g. /var/lib/m80-run on root fs).",
                run_root.display(),
            ),
        });
    }

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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use m80_image_manifest::{ImageKind, KernelKind, Manifest, ManifestError, SCHEMA_VERSION};

    use super::*;

    const SHA256_EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        key: &'static str,
        old: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            let old = env::var_os(key);
            env::set_var(key, value);
            Self { key, old }
        }

        fn set_str(key: &'static str, value: &str) -> Self {
            let old = env::var_os(key);
            env::set_var(key, value);
            Self { key, old }
        }

        fn remove(key: &'static str) -> Self {
            let old = env::var_os(key);
            env::remove_var(key);
            Self { key, old }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(old) = &self.old {
                env::set_var(self.key, old);
            } else {
                env::remove_var(self.key);
            }
        }
    }

    fn fixture_dir(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!("m80-preflight-{name}-{unique}"));
        std::fs::create_dir(&dir).unwrap();
        dir
    }

    fn write_empty(path: &Path) {
        std::fs::write(path, b"").unwrap();
    }

    fn minimal_manifest(dir: &Path) -> (PathBuf, Manifest) {
        let kernel = dir.join("vmlinux");
        let rootfs = dir.join("output.ext4");
        let daemon = dir.join("m80-guestd");
        write_empty(&kernel);
        write_empty(&rootfs);
        write_empty(&daemon);

        let manifest = Manifest {
            boot_target: None,
            daemon_binary_path: daemon,
            daemon_binary_sha256: SHA256_EMPTY.to_string(),
            expected_firecracker_version: "v1.15.1".to_string(),
            guest_port: 9001,
            image_kind: ImageKind::Minimal,
            kernel_image: kernel,
            kernel_image_sha256: SHA256_EMPTY.to_string(),
            kernel_kind: KernelKind::Stock,
            no_egress_reason: None,
            output_rootfs_image: rootfs.clone(),
            output_rootfs_sha256: SHA256_EMPTY.to_string(),
            ready_marker: "M80_READY".to_string(),
            schema_version: SCHEMA_VERSION,
            service_unit_path: None,
            service_unit_sha256: None,
            source_rootfs_image: None,
            source_rootfs_sha256: None,
            workspace_mount_path: None,
            workspace_mount_sha256: None,
        };
        let manifest_path = PathBuf::from(format!("{}.manifest.json", rootfs.display()));
        manifest.write(&manifest_path).unwrap();
        (rootfs, manifest)
    }

    #[test]
    fn rootfs_and_manifest_check_accepts_verified_artifacts() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = fixture_dir("manifest-ok");
        let (rootfs, expected) = minimal_manifest(&dir);
        let _env = EnvGuard::set(ENV_ROOTFS_IMAGE, &rootfs);
        let _kernel_kind = EnvGuard::remove(ENV_KERNEL_KIND);
        let mut report = Vec::new();

        let (actual_rootfs, actual_manifest) =
            check_rootfs_and_manifest(&mut report).expect("manifest must verify");

        assert_eq!(actual_rootfs, rootfs);
        assert_eq!(actual_manifest, expected);
        assert_eq!(report[0].label, "Rootfs + manifest");
        assert!(
            report[0].detail.contains("sha256 ok"),
            "report must mark sha verification: {:?}",
            report[0]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rootfs_and_manifest_check_rejects_tampered_artifacts() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = fixture_dir("manifest-tampered");
        let (rootfs, manifest) = minimal_manifest(&dir);
        std::fs::write(&manifest.kernel_image, b"tampered").unwrap();
        let _env = EnvGuard::set(ENV_ROOTFS_IMAGE, &rootfs);
        let _kernel_kind = EnvGuard::remove(ENV_KERNEL_KIND);
        let mut report = Vec::new();

        let err = check_rootfs_and_manifest(&mut report).unwrap_err();

        match err {
            PreflightError::Manifest(ManifestError::Sha256Mismatch { field, .. }) => {
                assert_eq!(field, "kernel_image");
            }
            other => panic!("expected manifest sha mismatch, got {other:?}"),
        }
        assert!(
            report.is_empty(),
            "failed verification must not append a success row"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rootfs_and_manifest_check_honors_kernel_kind_env() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = fixture_dir("manifest-kernel-kind");
        let (rootfs, _) = minimal_manifest(&dir);
        let _rootfs = EnvGuard::set(ENV_ROOTFS_IMAGE, &rootfs);
        let _kernel_kind = EnvGuard::set_str(ENV_KERNEL_KIND, "stripped");
        let mut report = Vec::new();

        let (_, actual_manifest) =
            check_rootfs_and_manifest(&mut report).expect("manifest must verify");

        assert_eq!(actual_manifest.kernel_kind, KernelKind::Stripped);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn invalid_kernel_kind_env_fails_closed() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = fixture_dir("manifest-bad-kernel-kind");
        let (rootfs, _) = minimal_manifest(&dir);
        let _rootfs = EnvGuard::set(ENV_ROOTFS_IMAGE, &rootfs);
        let _kernel_kind = EnvGuard::set_str(ENV_KERNEL_KIND, "tiny");
        let mut report = Vec::new();

        let err = check_rootfs_and_manifest(&mut report).unwrap_err();

        assert!(
            err.to_string()
                .contains("M80_KERNEL_KIND must be stock|stripped"),
            "unexpected error: {err}"
        );
        assert!(
            report.is_empty(),
            "failed kernel kind parsing must not append a success row"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
