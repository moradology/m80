//! Store-wide flock helpers.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use nix::fcntl::{Flock, FlockArg};

use crate::StoreError;

pub(crate) const LOCK_FILE_NAME: &str = ".image-store.lock";
pub(crate) const TEMPLATE_COORDINATION_LOCK_FILE_NAME: &str = ".image-template-coordination.lock";

pub(crate) struct StoreLock {
    #[allow(dead_code)]
    file: Flock<File>,
}

impl StoreLock {
    pub(crate) fn exclusive(root: &Path) -> Result<Self, StoreError> {
        Self::exclusive_named(root, LOCK_FILE_NAME)
    }

    pub(crate) fn shared(root: &Path) -> Result<Self, StoreError> {
        Self::shared_named(root, LOCK_FILE_NAME)
    }

    pub(crate) fn exclusive_named(root: &Path, name: &str) -> Result<Self, StoreError> {
        Self::lock(root, name, FlockArg::LockExclusive)
    }

    pub(crate) fn shared_named(root: &Path, name: &str) -> Result<Self, StoreError> {
        Self::lock(root, name, FlockArg::LockShared)
    }

    fn lock(root: &Path, name: &str, mode: FlockArg) -> Result<Self, StoreError> {
        let path = root.join(name);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)
            .map_err(|source| StoreError::Io {
                path: path.clone(),
                source,
            })?;
        let file = Flock::lock(file, mode).map_err(|(_, source)| StoreError::Io {
            path,
            source: std::io::Error::from(source),
        })?;
        Ok(Self { file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusive_lock_creates_lock_file() {
        let root = tempfile::tempdir().expect("root");

        let _lock = StoreLock::exclusive(root.path()).expect("exclusive lock");

        assert!(root.path().join(LOCK_FILE_NAME).is_file());
    }

    #[test]
    fn shared_lock_uses_same_lock_file() {
        let root = tempfile::tempdir().expect("root");

        let _lock = StoreLock::shared(root.path()).expect("shared lock");

        assert!(root.path().join(LOCK_FILE_NAME).is_file());
    }

    #[test]
    fn named_lock_uses_requested_file() {
        let root = tempfile::tempdir().expect("root");

        let _lock = StoreLock::exclusive_named(root.path(), TEMPLATE_COORDINATION_LOCK_FILE_NAME)
            .expect("named lock");

        assert!(root
            .path()
            .join(TEMPLATE_COORDINATION_LOCK_FILE_NAME)
            .is_file());
    }
}
