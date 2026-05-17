//! Reflink capability probing for `OverlayTemplateCloneMode::Auto`.

use std::collections::HashMap;
use std::fmt;
use std::io::Write as _;
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

#[cfg(all(
    target_os = "linux",
    not(target_env = "musl"),
    not(target_env = "ohos")
))]
use nix::sys::statfs::XFS_SUPER_MAGIC;
use nix::sys::statfs::{
    statfs, FsType, BTRFS_SUPER_MAGIC, EXT2_SUPER_MAGIC, EXT3_SUPER_MAGIC, EXT4_SUPER_MAGIC,
    FUSE_SUPER_MAGIC, OVERLAYFS_SUPER_MAGIC, TMPFS_MAGIC,
};

const ZFS_SUPER_MAGIC_RAW: i64 = 0x2fc12fc1;

static PROBE_CACHE: OnceLock<Mutex<HashMap<u64, ReflinkCapability>>> = OnceLock::new();

/// Reflink support decision for a host filesystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReflinkCapability {
    /// A same-directory FICLONE probe succeeded.
    Supported,
    /// The filesystem cannot use reflinks for the overlay-template clone.
    Unsupported {
        /// Typed reason reflinks are unavailable.
        reason: UnsupportedReason,
    },
    /// The probe itself could not complete.
    ProbeFailed {
        /// Failure detail from statfs, metadata, or temporary-file setup.
        reason: String,
    },
}

/// Typed reason reflinks are unavailable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnsupportedReason {
    /// `statfs` identified a filesystem kind that does not support reflinks.
    FsTypeKnownNoReflink(FsKind),
    /// The kernel rejected FICLONE with this errno.
    FicloneRejected(i32),
    /// Source and destination ended up on different devices.
    CrossDevice,
    /// Other unsupported condition not worth splitting yet.
    #[allow(dead_code)]
    Other(String),
}

impl fmt::Display for UnsupportedReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FsTypeKnownNoReflink(kind) => {
                write!(f, "{kind} is classified as non-reflink")
            }
            Self::FicloneRejected(errno) => write!(f, "FICLONE rejected with errno {errno}"),
            Self::CrossDevice => f.write_str("FICLONE rejected cross-device clone"),
            Self::Other(reason) => f.write_str(reason),
        }
    }
}

/// Filesystem kinds relevant to the reflink gate.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum FsKind {
    /// XFS.
    Xfs,
    /// Btrfs.
    Btrfs,
    /// ext2.
    Ext2,
    /// ext3.
    Ext3,
    /// ext4.
    Ext4,
    /// tmpfs.
    Tmpfs,
    /// overlayfs.
    Overlayfs,
    /// FUSE-backed filesystem.
    Fuse,
    /// OpenZFS on Linux.
    Zfs,
    /// A filesystem not classified by this probe.
    Unknown(i64),
}

impl fmt::Display for FsKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Xfs => f.write_str("xfs"),
            Self::Btrfs => f.write_str("btrfs"),
            Self::Ext2 => f.write_str("ext2"),
            Self::Ext3 => f.write_str("ext3"),
            Self::Ext4 => f.write_str("ext4"),
            Self::Tmpfs => f.write_str("tmpfs"),
            Self::Overlayfs => f.write_str("overlayfs"),
            Self::Fuse => f.write_str("fuse"),
            Self::Zfs => f.write_str("zfs"),
            Self::Unknown(raw) => write!(f, "unknown({raw:#x})"),
        }
    }
}

/// Probe reflink support for files created under `dir`.
pub(crate) fn probe_for(dir: &Path) -> ReflinkCapability {
    let stat = match statfs(dir) {
        Ok(stat) => stat,
        Err(err) => {
            return ReflinkCapability::ProbeFailed {
                reason: format!("statfs {} failed: {err}", dir.display()),
            }
        }
    };

    let fs_kind = fs_kind(stat.filesystem_type());
    if known_no_reflink(fs_kind) {
        return ReflinkCapability::Unsupported {
            reason: UnsupportedReason::FsTypeKnownNoReflink(fs_kind),
        };
    }

    probe_ficlone(dir)
}

/// Probe reflink support once per `st_dev` for the process lifetime.
pub(crate) fn probe_cached_for(parent_dir: &Path) -> ReflinkCapability {
    let dev_id = match device_id_for(parent_dir) {
        Ok(dev_id) => dev_id,
        Err(err) => {
            return ReflinkCapability::ProbeFailed {
                reason: format!("metadata {} failed: {err}", parent_dir.display()),
            }
        }
    };

    let cache = PROBE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().expect("reflink probe cache poisoned");
    guard
        .entry(dev_id)
        .or_insert_with(|| probe_for(parent_dir))
        .clone()
}

/// Device ID for the filesystem containing `path`.
pub(crate) fn device_id_for(path: &Path) -> std::io::Result<u64> {
    Ok(std::fs::metadata(path)?.dev())
}

fn fs_kind(fs_type: FsType) -> FsKind {
    if fs_type == BTRFS_SUPER_MAGIC {
        FsKind::Btrfs
    } else if is_xfs(fs_type) {
        FsKind::Xfs
    } else if fs_type == EXT2_SUPER_MAGIC {
        FsKind::Ext2
    } else if fs_type == EXT3_SUPER_MAGIC {
        FsKind::Ext3
    } else if fs_type == EXT4_SUPER_MAGIC {
        FsKind::Ext4
    } else if fs_type == TMPFS_MAGIC {
        FsKind::Tmpfs
    } else if fs_type == OVERLAYFS_SUPER_MAGIC {
        FsKind::Overlayfs
    } else if fs_type == FUSE_SUPER_MAGIC {
        FsKind::Fuse
    } else if fs_type.0 as i64 == ZFS_SUPER_MAGIC_RAW {
        FsKind::Zfs
    } else {
        FsKind::Unknown(fs_type.0 as i64)
    }
}

#[cfg(all(
    target_os = "linux",
    not(target_env = "musl"),
    not(target_env = "ohos")
))]
fn is_xfs(fs_type: FsType) -> bool {
    fs_type == XFS_SUPER_MAGIC
}

#[cfg(not(all(
    target_os = "linux",
    not(target_env = "musl"),
    not(target_env = "ohos")
)))]
fn is_xfs(_fs_type: FsType) -> bool {
    false
}

fn known_no_reflink(fs_kind: FsKind) -> bool {
    matches!(
        fs_kind,
        FsKind::Ext2
            | FsKind::Ext3
            | FsKind::Ext4
            | FsKind::Tmpfs
            | FsKind::Overlayfs
            | FsKind::Fuse
    )
}

fn probe_ficlone(dir: &Path) -> ReflinkCapability {
    let mut source = match tempfile::Builder::new()
        .prefix(".m80-reflink-probe-src-")
        .tempfile_in(dir)
    {
        Ok(file) => file,
        Err(err) => {
            return ReflinkCapability::ProbeFailed {
                reason: format!("create FICLONE source in {} failed: {err}", dir.display()),
            }
        }
    };
    if let Err(err) = source.as_file_mut().write_all(b"m80 reflink probe\n") {
        return ReflinkCapability::ProbeFailed {
            reason: format!("write FICLONE source in {} failed: {err}", dir.display()),
        };
    }

    let dest = match tempfile::Builder::new()
        .prefix(".m80-reflink-probe-dst-")
        .tempfile_in(dir)
    {
        Ok(file) => file,
        Err(err) => {
            return ReflinkCapability::ProbeFailed {
                reason: format!("create FICLONE dest in {} failed: {err}", dir.display()),
            }
        }
    };

    match rustix::fs::ioctl_ficlone(dest.as_file(), source.as_file()) {
        Ok(()) => ReflinkCapability::Supported,
        Err(err) if err.raw_os_error() == nix::libc::EXDEV => ReflinkCapability::Unsupported {
            reason: UnsupportedReason::CrossDevice,
        },
        Err(err) => ReflinkCapability::Unsupported {
            reason: UnsupportedReason::FicloneRejected(err.raw_os_error()),
        },
    }
}
