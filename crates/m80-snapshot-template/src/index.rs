//! Store index schema.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::wrap_io;
use crate::{TemplateFingerprint, TemplateStoreError, SCHEMA_VERSION};

/// Store index persisted at `<store-root>/index.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Index {
    /// Template entries known to the store.
    pub entries: Vec<IndexEntry>,
    /// Schema version. v0.1 = `1`.
    pub schema_version: u32,
}

/// One entry in the snapshot-template store index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexEntry {
    /// Template fingerprint.
    pub fingerprint: TemplateFingerprint,
    /// Last successful pin or commit timestamp, in Unix epoch milliseconds.
    pub last_used_unix_ms: u64,
    /// Size in bytes of `vm.snap`, `mem.snap`, and `manifest.json`.
    pub size_bytes: u64,
}

impl Index {
    /// Return an empty index at the active schema version.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
            schema_version: SCHEMA_VERSION,
        }
    }

    /// Parse an index from raw bytes, probing `schema_version` before full parse.
    pub fn from_bytes(raw: &[u8]) -> Result<Self, TemplateStoreError> {
        parse_with_schema_probe(raw, Path::new("index.json"))
    }

    /// Read and validate an index file.
    pub fn read(path: &Path) -> Result<Self, TemplateStoreError> {
        let raw = std::fs::read(path).map_err(wrap_io(path))?;
        parse_with_schema_probe(&raw, path)
    }

    /// Write this index as pretty JSON with a trailing newline.
    pub fn write(&self, path: &Path) -> Result<(), TemplateStoreError> {
        write_pretty_json(self, path)
    }

    pub(crate) fn upsert(&mut self, entry: IndexEntry) {
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|existing| existing.fingerprint == entry.fingerprint)
        {
            *existing = entry;
            return;
        }
        self.entries.push(entry);
    }

    pub(crate) fn remove(&mut self, fingerprint: &TemplateFingerprint) {
        self.entries
            .retain(|entry| &entry.fingerprint != fingerprint);
    }
}

#[derive(Deserialize)]
struct SchemaVersionProbe {
    schema_version: u32,
}

fn parse_with_schema_probe(raw: &[u8], path: &Path) -> Result<Index, TemplateStoreError> {
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
        path: path.to_path_buf(),
        source,
    })
}

pub(crate) fn write_pretty_json<T: Serialize>(
    value: &T,
    path: &Path,
) -> Result<(), TemplateStoreError> {
    let mut json =
        serde_json::to_string_pretty(value).map_err(|source| TemplateStoreError::Json {
            path: path.to_path_buf(),
            source,
        })?;
    json.push('\n');
    std::fs::write(path, json.as_bytes()).map_err(wrap_io(path))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644))
            .map_err(wrap_io(path))?;
    }
    Ok(())
}
