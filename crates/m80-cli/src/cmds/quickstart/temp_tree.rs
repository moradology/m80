use std::fs;
use std::path::{Path, PathBuf};

use m80_firecracker::FcError;

pub(super) struct TempTree {
    path: PathBuf,
}

impl TempTree {
    pub(super) fn new() -> Result<Self, FcError> {
        let path = std::env::temp_dir().join(format!("m80-quickstart-{}", ulid::Ulid::new()));
        fs::create_dir(&path).map_err(|e| FcError::PathIo {
            path: path.clone(),
            source: e,
        })?;
        Ok(Self { path })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
