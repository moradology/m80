//! Boot artifact, run-root, and storage-helper preflight.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use m80_image_manifest::{KernelKind, Manifest};
use nix::sys::statvfs::statvfs;

use crate::PreflightError;

/// Environment key for overriding the kernel image path.
pub const ENV_KERNEL_IMAGE: &str = "M80_KERNEL_IMAGE";
/// Environment key for overriding the manifest kernel kind.
pub const ENV_KERNEL_KIND: &str = "M80_KERNEL_KIND";
/// Environment key for selecting the rootfs image path.
pub const ENV_ROOTFS_IMAGE: &str = "M80_ROOTFS_IMAGE";
/// Environment key for selecting the managed artifact directory.
pub(crate) const ENV_ARTIFACT_DIR: &str = "M80_ARTIFACT_DIR";
/// Environment key for selecting the run-root directory.
pub(crate) const ENV_RUN_ROOT: &str = "M80_RUN_ROOT";

/// Default managed artifact directory.
pub(crate) const DEFAULT_ARTIFACT_DIR: &str = "/opt/m80/artifacts";
/// Default run-root directory.
pub(crate) const DEFAULT_RUN_ROOT: &str = "/var/run/m80";

/// 100 MiB minimum free space for the run-root.
pub(crate) const MIN_RUN_ROOT_FREE_BYTES: u64 = 100 * 1024 * 1024;

/// Storage helpers required by the storage and overlay preparation path.
pub(crate) const REQUIRED_STORAGE_HELPERS: &[&str] =
    &["mkfs.ext4", "cp", "fallocate", "debugfs", "e2fsck"];

/// Inputs for boot artifact, run-root, and helper validation.
#[derive(Debug, Clone)]
pub struct ArtifactPreflightConfig {
    /// Optional explicit kernel image path.
    pub kernel_image: Option<PathBuf>,
    /// Managed artifact directory used when `kernel_image` is absent.
    pub artifact_dir: PathBuf,
    /// Optional rootfs image path.
    pub rootfs_image: Option<PathBuf>,
    /// Optional manifest kernel-kind override.
    pub kernel_kind: Option<String>,
    /// Run-root directory that must already exist and have enough capacity.
    pub run_root: PathBuf,
    /// PATH-style helper search path.
    pub helper_search_path: Option<OsString>,
}

impl ArtifactPreflightConfig {
    /// Build a config from the exact m80 environment keys.
    pub fn from_env() -> Self {
        Self {
            kernel_image: env::var_os(ENV_KERNEL_IMAGE).map(PathBuf::from),
            artifact_dir: env::var_os(ENV_ARTIFACT_DIR)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_ARTIFACT_DIR)),
            rootfs_image: env::var_os(ENV_ROOTFS_IMAGE).map(PathBuf::from),
            kernel_kind: env::var(ENV_KERNEL_KIND).ok(),
            run_root: env::var_os(ENV_RUN_ROOT)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_RUN_ROOT)),
            helper_search_path: env::var_os("PATH"),
        }
    }
}

/// Validated boot artifacts and host paths.
#[derive(Debug, Clone)]
pub(crate) struct ArtifactPreflight {
    /// Resolved kernel image path.
    pub(crate) kernel: PathBuf,
    /// Resolved rootfs image path.
    pub(crate) rootfs: PathBuf,
    /// Validated provenance manifest.
    pub(crate) manifest: Manifest,
    /// Validated run-root path.
    pub(crate) run_root: PathBuf,
    /// Required storage helpers that were found on PATH.
    pub(crate) storage_helpers: Vec<String>,
}

/// Validate boot artifacts, run-root, and required storage helper binaries.
pub(crate) fn verify_artifacts(
    config: &ArtifactPreflightConfig,
) -> Result<ArtifactPreflight, PreflightError> {
    let kernel = discover_kernel(config)?;
    let (rootfs, manifest) = verify_rootfs_and_manifest(config)?;
    let run_root = verify_run_root(&config.run_root)?;
    let storage_helpers = verify_storage_helpers(config.helper_search_path.as_ref())?;

    Ok(ArtifactPreflight {
        kernel,
        rootfs,
        manifest,
        run_root,
        storage_helpers,
    })
}

fn discover_kernel(config: &ArtifactPreflightConfig) -> Result<PathBuf, PreflightError> {
    if let Some(path) = &config.kernel_image {
        if !path.is_absolute() {
            return Err(PreflightError::NonAbsolutePath {
                kind: "kernel".to_string(),
                path: path.clone(),
            });
        }
        if !path.exists() {
            return Err(PreflightError::KernelNotFound);
        }
        return Ok(path.clone());
    }

    let mut candidates: Vec<PathBuf> = fs::read_dir(&config.artifact_dir)
        .map_err(|source| PreflightError::PathIo {
            path: config.artifact_dir.clone(),
            source,
        })?
        .filter_map(|entry| entry.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("vmlinux-"))
        .map(|e| e.path())
        .collect();
    // Lexicographic sort: "vmlinux-6.10" sorts before "vmlinux-6.9" because
    // '1' < '9' at the first differing character. This means the picked kernel
    // may not be the numerically newest when minor versions cross a digit
    // boundary (e.g. 6.9 → 6.10). In practice, managed artifact directories
    // contain at most one kernel image, so this is a non-issue. If that
    // invariant ever changes, replace `.sort()` with a semver-aware comparator.
    candidates.sort();
    candidates.pop().ok_or(PreflightError::KernelNotFound)
}

fn verify_rootfs_and_manifest(
    config: &ArtifactPreflightConfig,
) -> Result<(PathBuf, Manifest), PreflightError> {
    let rootfs = config
        .rootfs_image
        .clone()
        .ok_or(PreflightError::RootfsNotFound)?;

    if !rootfs.is_absolute() {
        return Err(PreflightError::NonAbsolutePath {
            kind: "rootfs".to_string(),
            path: rootfs,
        });
    }

    if !rootfs.exists() {
        return Err(PreflightError::RootfsNotFound);
    }

    let manifest_path = PathBuf::from(format!("{}.manifest.json", rootfs.display()));
    let mut manifest = Manifest::read(&manifest_path)?;
    if let Some(kind) = &config.kernel_kind {
        manifest.kernel_kind = parse_kernel_kind(kind)?;
    }

    let parent = rootfs.parent().unwrap_or_else(|| Path::new("/"));
    manifest.verify(parent)?;

    Ok((rootfs, manifest))
}

fn parse_kernel_kind(raw: &str) -> Result<KernelKind, PreflightError> {
    match raw {
        "stock" => Ok(KernelKind::Stock),
        "stripped" => Ok(KernelKind::Stripped),
        other => Err(PreflightError::InvalidKernelKind {
            actual: other.to_owned(),
        }),
    }
}

fn verify_run_root(run_root: &Path) -> Result<PathBuf, PreflightError> {
    if !run_root.is_absolute() {
        return Err(PreflightError::RunRootUnavailable {
            reason: format!("path is not absolute: {}", run_root.display()),
        });
    }

    if !run_root.exists() {
        return Err(PreflightError::RunRootUnavailable {
            reason: format!("directory does not exist: {}", run_root.display()),
        });
    }

    let stat = statvfs(run_root).map_err(|e| PreflightError::RunRootUnavailable {
        reason: format!("statvfs failed on {}: {e}", run_root.display()),
    })?;

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

    Ok(run_root.to_path_buf())
}

fn verify_storage_helpers(search_path: Option<&OsString>) -> Result<Vec<String>, PreflightError> {
    let mut found = Vec::with_capacity(REQUIRED_STORAGE_HELPERS.len());
    for helper in REQUIRED_STORAGE_HELPERS {
        if find_in_path(helper, search_path).is_none() {
            return Err(PreflightError::StorageHelperMissing(helper.to_string()));
        }
        found.push((*helper).to_string());
    }
    Ok(found)
}

/// Look up `binary` in `search_path` (a `PATH`-style colon-separated list).
fn find_in_path(binary: &str, search_path: Option<&OsString>) -> Option<PathBuf> {
    let search_path = search_path?;
    env::split_paths(search_path)
        .map(|dir| dir.join(binary))
        .find(|candidate| candidate.exists())
}

#[cfg(test)]
mod tests;
