//! Store path layout helpers.

use std::path::{Path, PathBuf};

use m80_snapshot::SnapshotPaths;

use crate::{TemplateFingerprint, TemplateStoreError};

pub(crate) const BY_FINGERPRINT_DIR: &str = "by-fingerprint";
pub(crate) const INDEX_FILE: &str = "index.json";
pub(crate) const MANIFEST_FILE: &str = "manifest.json";
pub(crate) const MEM_FILE: &str = "mem.snap";
pub(crate) const STAGING_DIR: &str = "staging";
pub(crate) const VM_STATE_FILE: &str = "vm.snap";

/// Paths to one template body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateBodyPaths {
    /// Host path to the Firecracker VM-state snapshot.
    pub vm_state: PathBuf,
    /// Host path to the Firecracker memory snapshot.
    pub mem: PathBuf,
    /// Host path to the template manifest.
    pub manifest: PathBuf,
}

impl TemplateBodyPaths {
    /// Return the snapshot pair paths.
    #[must_use]
    pub fn snapshot_paths(&self) -> SnapshotPaths {
        SnapshotPaths {
            vm_state: self.vm_state.clone(),
            mem: self.mem.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct StoreLayout {
    root: PathBuf,
}

impl StoreLayout {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn index_path(&self) -> PathBuf {
        self.root.join(INDEX_FILE)
    }

    pub(crate) fn by_fingerprint_dir(&self) -> PathBuf {
        self.root.join(BY_FINGERPRINT_DIR)
    }

    pub(crate) fn staging_root(&self) -> PathBuf {
        self.root.join(STAGING_DIR)
    }

    pub(crate) fn template_dir(&self, fingerprint: &TemplateFingerprint) -> PathBuf {
        self.by_fingerprint_dir().join(fingerprint.to_hex())
    }

    pub(crate) fn staging_dir(&self, fingerprint: &TemplateFingerprint, sequence: u64) -> PathBuf {
        self.staging_root().join(format!(
            "{}-{}-{sequence}",
            fingerprint.to_hex(),
            std::process::id()
        ))
    }

    pub(crate) fn body_paths(&self, dir: &Path) -> TemplateBodyPaths {
        TemplateBodyPaths {
            vm_state: dir.join(VM_STATE_FILE),
            mem: dir.join(MEM_FILE),
            manifest: dir.join(MANIFEST_FILE),
        }
    }

    pub(crate) fn ensure_new_store_dirs(&self) -> Result<(), TemplateStoreError> {
        ensure_root_parent_exists(&self.root)?;
        if !self.root.exists() {
            std::fs::create_dir(&self.root).map_err(crate::error::wrap_io(&self.root))?;
        }
        if !self.root.is_dir() {
            return Err(TemplateStoreError::InvalidStoreRoot {
                path: self.root.clone(),
                reason: "root must be a directory",
            });
        }
        let by_fingerprint = self.by_fingerprint_dir();
        if !by_fingerprint.exists() {
            std::fs::create_dir(&by_fingerprint).map_err(crate::error::wrap_io(&by_fingerprint))?;
        }
        let staging = self.staging_root();
        if !staging.exists() {
            std::fs::create_dir(&staging).map_err(crate::error::wrap_io(&staging))?;
        }
        Ok(())
    }

    pub(crate) fn ensure_existing_store_dirs(&self) -> Result<(), TemplateStoreError> {
        for path in [
            self.root.clone(),
            self.by_fingerprint_dir(),
            self.staging_root(),
        ] {
            if !path.is_dir() {
                return Err(TemplateStoreError::InvalidStoreRoot {
                    path,
                    reason: "required store directory is missing",
                });
            }
        }
        if !self.index_path().is_file() {
            return Err(TemplateStoreError::InvalidStoreRoot {
                path: self.index_path(),
                reason: "index.json is missing",
            });
        }
        Ok(())
    }
}

fn ensure_root_parent_exists(root: &Path) -> Result<(), TemplateStoreError> {
    let Some(parent) = root.parent() else {
        return Err(TemplateStoreError::InvalidStoreRoot {
            path: root.to_path_buf(),
            reason: "root must have a parent directory",
        });
    };
    if !parent.is_dir() {
        return Err(TemplateStoreError::InvalidStoreRoot {
            path: root.to_path_buf(),
            reason: "root parent directory does not exist",
        });
    }
    Ok(())
}
