//! Per-VM rootfs overlay allocation, scratch image hydration, and opt-in
//! post-stop change extraction.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epic `m80-urc` (`br show m80-urc`).

#![deny(missing_docs)]

use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

mod rootfs;
mod scratch;

pub use rootfs::Rootfs;
pub use scratch::Scratch;

/// Result of a successful [`Scratch::extract`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeSet {
    /// Files that survived the admissibility scan and were staged into the
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
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum RejectionReason {
    /// Path was a symlink.
    Symlink,
    /// Path was a special file (device, fifo, socket).
    SpecialFile,
    /// Other reason; free text. Used for unsupported file types not covered
    /// by the more-specific variants above.
    Other(String),
}

/// Errors surfaced by storage operations.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// Sparse overlay file could not be created or sized.
    ///
    /// Covers: `File::create` on a missing parent dir, `set_len` failures,
    /// and failure to spawn `mkfs.ext4`.
    #[error("overlay create failed at {}: {err}", path.display())]
    OverlayCreateFailed {
        /// Path of the overlay file that could not be created.
        path: PathBuf,
        /// Underlying I/O error.
        err: io::Error,
    },
    /// `mkfs.ext4 -F` exited with a non-zero status.
    #[error("mkfs.ext4 failed on {} (exit {status}): {stderr}", path.display())]
    MkfsFailed {
        /// Path of the overlay file being formatted.
        path: PathBuf,
        /// Exit code from `mkfs.ext4` (`-1` if the process was signalled).
        status: i32,
        /// Captured stderr (or stdout if stderr was empty).
        stderr: String,
    },
    /// `mkfs.ext4` failed during scratch image creation.
    #[error("mkfs.ext4 failed: {0}")]
    Mkfs(io::Error),
    /// `e2fsck` exited with a fatal code.
    #[error("e2fsck exit={exit}: {stderr}")]
    E2fsckFailed {
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
    /// Underlying I/O failure; carries the path so the caller doesn't have to
    /// guess which file failed.
    #[error("i/o on {}: {source}", path.display())]
    Io {
        /// File the I/O was attempted against.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
}

/// Map an [`io::Error`] to [`StorageError::Io`] carrying `path`.
pub(crate) fn io_err(path: impl Into<PathBuf>, source: io::Error) -> StorageError {
    StorageError::Io {
        path: path.into(),
        source,
    }
}
