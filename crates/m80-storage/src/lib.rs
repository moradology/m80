//! Per-VM rootfs overlay allocation, scratch image hydration, and opt-in
//! post-stop change extraction.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epic `m80-urc` (`br show m80-urc`).

#![deny(missing_docs)]

use std::io;
use std::os::unix::process::ExitStatusExt as _;
use std::path::PathBuf;
use std::process::ExitStatus;

use serde::{Deserialize, Serialize};

mod rootfs;
mod scratch;

pub use rootfs::Rootfs;
pub use scratch::Scratch;

/// Result of a successful [`Scratch::extract`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct Rejection {
    /// Path that was rejected (relative to the scratch root).
    pub path: PathBuf,
    /// Reason for rejection.
    pub reason: RejectionReason,
}

/// Why an extracted entry was refused.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    deny_unknown_fields,
    tag = "kind",
    content = "detail",
    rename_all = "snake_case"
)]
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
    /// Overlay template creation or lock acquisition failed.
    #[error("overlay template create failed at {}: {err}", path.display())]
    OverlayTemplateCreateFailed {
        /// Template or lock path that failed.
        path: PathBuf,
        /// Underlying I/O error.
        err: io::Error,
    },
    /// Existing overlay template metadata did not match the requested shape.
    #[error("overlay template metadata mismatch at {}: {reason}", path.display())]
    OverlayTemplateMismatch {
        /// Template metadata path.
        path: PathBuf,
        /// Human-readable mismatch reason.
        reason: String,
    },
    /// Cloning the overlay template to the per-VM overlay failed.
    #[error("overlay template clone failed from {} to {}: {err}", template.display(), dest.display())]
    OverlayTemplateCloneFailed {
        /// Source template path.
        template: PathBuf,
        /// Destination per-VM overlay path.
        dest: PathBuf,
        /// Underlying I/O error.
        err: io::Error,
    },
    /// A storage subprocess (mkfs.ext4, e2fsck, etc.) exited non-zero.
    #[error("{program} failed on {} (exit {status}): {stderr}", path.display())]
    SubprocessFailed {
        /// Name of the program that failed (e.g. `"mkfs.ext4"`, `"e2fsck"`).
        program: &'static str,
        /// Path the subprocess was operating on.
        path: PathBuf,
        /// Formatted exit status.
        status: String,
        /// Captured stderr (or stdout if stderr was empty).
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

/// Format a process exit status for display in error messages.
///
/// Returns the numeric exit code as a string, or `"signal: <N>"` when the
/// process was terminated by a signal and `ExitStatus::code()` is `None`.
pub(crate) fn format_exit(status: ExitStatus) -> String {
    if let Some(code) = status.code() {
        code.to_string()
    } else if let Some(sig) = status.signal() {
        format!("signal: {sig}")
    } else {
        "unknown".to_owned()
    }
}

/// Map an [`io::Error`] to [`StorageError::Io`] carrying `path`.
pub(crate) fn io_err(path: impl Into<PathBuf>, source: io::Error) -> StorageError {
    StorageError::Io {
        path: path.into(),
        source,
    }
}
