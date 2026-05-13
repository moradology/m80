use std::os::unix::prelude::AsRawFd;
use std::path::Path;

use m80_snapshot::SnapshotPaths;

use crate::error::FcError;

pub(super) fn prime_snapshot_files(paths: &SnapshotPaths) -> Result<(), FcError> {
    prime_snapshot_file(&paths.vm_state)?;
    prime_snapshot_file(&paths.mem)?;
    Ok(())
}

fn prime_snapshot_file(path: &Path) -> Result<(), FcError> {
    let file = std::fs::File::open(path).map_err(|source| FcError::PathIo {
        path: path.to_path_buf(),
        source,
    })?;
    nix::fcntl::posix_fadvise(
        file.as_raw_fd(),
        0,
        0,
        nix::fcntl::PosixFadviseAdvice::POSIX_FADV_WILLNEED,
    )
    .map_err(|errno| FcError::PathIo {
        path: path.to_path_buf(),
        source: std::io::Error::from_raw_os_error(errno as i32),
    })
}
