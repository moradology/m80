//! Template manifest schema.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::wrap_io;
use crate::index;
use crate::{
    JailBackingPath, PmemTemplateEntry, TemplateFingerprint, TemplateInputs, TemplateStoreError,
    SCHEMA_VERSION,
};

/// Manifest persisted at `<store-root>/by-fingerprint/<hex>/manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateManifest {
    /// Template fingerprint recorded at commit time.
    pub fingerprint: TemplateFingerprint,
    /// Typed inputs used to compute the fingerprint.
    pub inputs: TemplateInputs,
    /// Restore layout metadata consumed by later Firecracker orchestration.
    pub restore_layout: TemplateRestoreLayout,
    /// Schema version. v0.1 = `1`.
    pub schema_version: u32,
    /// Snapshot-pair manifest for `vm.snap` and `mem.snap`.
    pub snapshot_manifest: m80_snapshot::SnapshotManifest,
}

impl TemplateManifest {
    /// Parse a template manifest from raw bytes.
    pub fn from_bytes(raw: &[u8]) -> Result<Self, TemplateStoreError> {
        parse_manifest_with_schema_probe(raw, Path::new("manifest.json"))
    }

    /// Read and validate a template manifest from `path`.
    pub fn read(path: &Path) -> Result<Self, TemplateStoreError> {
        let raw = std::fs::read(path).map_err(wrap_io(path))?;
        parse_manifest_with_schema_probe(&raw, path)
    }

    /// Write this manifest as pretty JSON with a trailing newline.
    pub fn write(&self, path: &Path) -> Result<(), TemplateStoreError> {
        index::write_pretty_json(self, path)
    }
}

/// Stable restore layout recorded in the template manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateRestoreLayout {
    /// Jail-visible path to the memory snapshot body.
    pub jail_mem_path: JailBackingPath,
    /// Jail-visible path to the VM-state snapshot body.
    pub jail_vm_state_path: JailBackingPath,
    /// Pmem backing paths expected at restore.
    pub pmem_backings: Vec<PmemTemplateEntry>,
}

impl TemplateRestoreLayout {
    /// Construct restore layout metadata from validated parts.
    #[must_use]
    pub fn new(
        jail_vm_state_path: JailBackingPath,
        jail_mem_path: JailBackingPath,
        pmem_backings: Vec<PmemTemplateEntry>,
    ) -> Self {
        Self {
            jail_mem_path,
            jail_vm_state_path,
            pmem_backings,
        }
    }
}

fn parse_manifest_with_schema_probe(
    raw: &[u8],
    path: &Path,
) -> Result<TemplateManifest, TemplateStoreError> {
    #[derive(Deserialize)]
    struct SchemaVersionProbe {
        schema_version: u32,
    }
    let probe: SchemaVersionProbe =
        serde_json::from_slice(raw).map_err(|source| TemplateStoreError::Json {
            path: path.to_path_buf(),
            source,
        })?;
    if probe.schema_version != SCHEMA_VERSION {
        return Err(TemplateStoreError::UnsupportedSchemaVersion {
            got: probe.schema_version,
            expected: SCHEMA_VERSION,
        });
    }
    serde_json::from_slice(raw).map_err(|source| TemplateStoreError::Json {
        path: PathBuf::from(path),
        source,
    })
}
