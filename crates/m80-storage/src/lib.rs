//! Per-VM rootfs cloning, scratch image hydration, and opt-in post-stop
//! change extraction (e2fsck/debugfs).
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epic `m80-urc` (`br show m80-urc`).
//!
//! # Type-pinning pass
//!
//! Public surface is declared here; bodies are `todo!()`. Implementation lands
//! in a later wave.

#![deny(missing_docs)]

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A per-VM rootfs ext4 clone.
#[derive(Debug)]
pub struct Rootfs {
    _path: PathBuf,
}

impl Rootfs {
    /// Clone the immutable base ext4 at `base` into a per-VM clone at
    /// `dest`. The base sha256 is verified against the manifest before
    /// cloning; mismatch is fatal.
    pub fn clone(_base: &Path, _dest: &Path) -> Result<Self, StorageError> {
        todo!()
    }

    /// Path of this clone.
    pub fn path(&self) -> &Path {
        todo!()
    }
}

/// A per-VM scratch ext4 image.
#[derive(Debug)]
pub struct Scratch {
    _path: PathBuf,
}

impl Scratch {
    /// Format a scratch ext4 at `image` of size `size` bytes and hydrate it
    /// from the host workspace tree at `workspace`.
    pub fn create(_workspace: &Path, _image: &Path, _size: u64) -> Result<Self, StorageError> {
        todo!()
    }

    /// Post-stop extraction: `e2fsck` repair → `debugfs` enumerate →
    /// admissibility scan → staging tree → atomic swap into `into`.
    /// Failure at any step rolls back fully.
    pub fn extract(_image: &Path, _into: &Path) -> Result<ChangeSet, StorageError> {
        todo!()
    }

    /// Path of this scratch image.
    pub fn path(&self) -> &Path {
        todo!()
    }
}

/// Result of a successful [`Scratch::extract`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeSet {
    /// Files that survived the admissibility scan and were swapped into the
    /// destination.
    pub staged: Vec<PathBuf>,
    /// Entries that were rejected by the admissibility scan.
    pub rejected: Vec<Rejection>,
    /// Total bytes of `staged`.
    pub total_bytes: u64,
}

/// One rejected entry from the admissibility scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rejection {
    /// Path that was rejected (relative to the scratch root).
    pub path: PathBuf,
    /// Reason for rejection.
    pub reason: RejectionReason,
}

/// Why an extracted entry was refused.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RejectionReason {
    /// Path was a symlink.
    Symlink,
    /// Path was a special file (device, fifo, socket).
    SpecialFile,
    /// Path resolved outside the workspace root.
    OutsideWorkspace,
    /// Other reason; free text.
    Other(String),
}

/// Errors surfaced by storage operations.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// The base rootfs's sha256 did not match the manifest.
    #[error("base rootfs sha256 mismatch")]
    BaseSha256Mismatch,
    /// `mkfs.ext4` failed.
    #[error("mkfs.ext4 failed: {0}")]
    Mkfs(io::Error),
    /// `e2fsck` failed.
    #[error("e2fsck exit={exit}: {stderr}")]
    E2fsckFailed {
        /// Process exit code.
        exit: i32,
        /// Captured stderr (best-effort UTF-8).
        stderr: String,
    },
    /// `debugfs` failed.
    #[error("debugfs exit={exit}: {stderr}")]
    DebugfsFailed {
        /// Process exit code.
        exit: i32,
        /// Captured stderr (best-effort UTF-8).
        stderr: String,
    },
    /// The admissibility scan refused the change set.
    #[error("admissibility scan refused the change set")]
    AdmissibilityRefused,
    /// The atomic swap into the destination failed.
    #[error("atomic swap into destination failed")]
    SwapFailed,
    /// Underlying I/O failure.
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
}
