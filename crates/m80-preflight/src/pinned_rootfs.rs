use std::fs::File;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Rootfs artifact opened and held by preflight after sha256 verification.
#[derive(Debug, Clone)]
pub struct PinnedRootfs {
    path: PathBuf,
    file: Arc<File>,
}

impl PinnedRootfs {
    /// Build a pinned rootfs handle from an already-open file.
    ///
    /// The caller is responsible for verifying the file contents before
    /// placing this handle in a preflight discovery.
    #[must_use]
    pub fn from_file(path: PathBuf, file: File) -> Self {
        Self {
            path,
            file: Arc::new(file),
        }
    }

    /// Original resolved rootfs path used for diagnostics and identity files.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Process-qualified procfs path for this pinned rootfs descriptor.
    ///
    /// The path uses `/proc/<pid>/fd/<fd>` rather than `/proc/self/fd/<fd>`
    /// because launch passes it through subprocess and mount-planning
    /// boundaries. Consumers can open or bind this path while this handle is
    /// alive without re-resolving the original rootfs pathname.
    #[must_use]
    pub fn proc_fd_path(&self) -> PathBuf {
        PathBuf::from(format!(
            "/proc/{}/fd/{}",
            std::process::id(),
            self.file.as_raw_fd()
        ))
    }
}
