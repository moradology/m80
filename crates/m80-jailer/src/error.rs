//! [`JailerError`] sum.

use std::io;
use std::path::PathBuf;

/// Errors surfaced by jailer operations.
#[derive(Debug, thiserror::Error)]
pub enum JailerError {
    /// A bind-mount failed.
    #[error("bind-mount failed: src={src} dest={dest}", src = src.display(), dest = dest.display())]
    BindFailed {
        /// Host source path that failed.
        src: PathBuf,
        /// In-jail destination path that failed.
        dest: PathBuf,
    },
    /// `chroot` syscall (or jailer's chroot step) failed.
    #[error("chroot failed in {jail_path}", jail_path = jail_path.display())]
    ChrootFailed {
        /// Jail path that failed to chroot.
        jail_path: PathBuf,
    },
    /// `firecracker.pid` did not appear within the poll deadline after launch.
    #[error("timed out waiting for firecracker.pid in {jail_path}", jail_path = jail_path.display())]
    FirecrackerPidTimeout {
        /// Jail path where `firecracker.pid` was expected.
        jail_path: PathBuf,
    },
    /// UID/GID was rejected (out of range or unknown).
    #[error("invalid uid/gid: uid={uid} gid={gid}")]
    UidGidInvalid {
        /// UID that was rejected.
        uid: u32,
        /// GID that was rejected.
        gid: u32,
    },
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
