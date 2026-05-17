//! Snapshot-template store implementation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use m80_snapshot::{
    Artifact, ArtifactKind, SnapshotManifest, SnapshotPaths, SNAPSHOT_MANIFEST_FILE,
};
use sha2::{Digest as _, Sha256};

use crate::error::wrap_io;
use crate::index::{Index, IndexEntry};
use crate::layout::StoreLayout;
use crate::{
    TemplateBodyPaths, TemplateFingerprint, TemplateInputs, TemplateManifest, TemplateRef,
    TemplateRestoreLayout, TemplateStoreError, SCHEMA_VERSION,
};

/// Content-addressed snapshot-template store.
#[derive(Debug)]
pub struct TemplateStore {
    layout: StoreLayout,
    capacity: usize,
    pins: Arc<Mutex<PinState>>,
    sequence: AtomicU64,
}

/// One snapshot-template entry recorded in the store index.
#[derive(Debug)]
pub struct TemplateSummary {
    /// Template fingerprint.
    pub fingerprint: TemplateFingerprint,
    /// Last successful pin or commit timestamp, in Unix epoch milliseconds.
    pub last_used_unix_ms: u64,
    /// Size in bytes of `vm.snap`, `mem.snap`, and `manifest.json`.
    pub size_bytes: u64,
}

/// Build reservation for a cache miss.
#[derive(Debug)]
pub struct TemplateBuildPlan {
    fingerprint: TemplateFingerprint,
    staging_dir: PathBuf,
    body_paths: TemplateBodyPaths,
    inputs: TemplateInputs,
    restore_layout: TemplateRestoreLayout,
}

impl TemplateBuildPlan {
    /// Template fingerprint being built.
    #[must_use]
    pub fn fingerprint(&self) -> &TemplateFingerprint {
        &self.fingerprint
    }

    /// Staging directory not visible through the content-addressed template path.
    #[must_use]
    pub fn staging_dir(&self) -> &Path {
        &self.staging_dir
    }

    /// Body paths where the caller must write `vm.snap` and `mem.snap`.
    #[must_use]
    pub fn body_paths(&self) -> &TemplateBodyPaths {
        &self.body_paths
    }

    /// Typed fingerprint inputs.
    #[must_use]
    pub fn inputs(&self) -> &TemplateInputs {
        &self.inputs
    }

    /// Restore layout metadata.
    #[must_use]
    pub fn restore_layout(&self) -> &TemplateRestoreLayout {
        &self.restore_layout
    }
}

/// A template pinned in the current process.
#[derive(Debug)]
pub struct PinnedTemplate {
    reference: TemplateRef,
    body_paths: TemplateBodyPaths,
    manifest: TemplateManifest,
    _pin: TemplatePin,
}

impl PinnedTemplate {
    /// Template reference suitable for hand-off to the orchestrator.
    #[must_use]
    pub fn reference(&self) -> &TemplateRef {
        &self.reference
    }

    /// Host-visible body paths for the pinned template.
    #[must_use]
    pub fn body_paths(&self) -> &TemplateBodyPaths {
        &self.body_paths
    }

    /// Parsed template manifest.
    #[must_use]
    pub fn manifest(&self) -> &TemplateManifest {
        &self.manifest
    }
}

/// Process-local RAII pin. Dropping releases the pin.
#[derive(Debug)]
pub struct TemplatePin {
    fingerprint: TemplateFingerprint,
    pins: Arc<Mutex<PinState>>,
}

impl TemplatePin {
    /// Fingerprint held by this pin.
    #[must_use]
    pub fn fingerprint(&self) -> &TemplateFingerprint {
        &self.fingerprint
    }
}

impl Drop for TemplatePin {
    fn drop(&mut self) {
        let mut pins = self.pins.lock().expect("template pin mutex poisoned");
        pins.release(&self.fingerprint);
    }
}

#[derive(Debug, Default)]
struct PinState {
    counts: HashMap<TemplateFingerprint, usize>,
}

impl PinState {
    fn acquire(&mut self, fingerprint: TemplateFingerprint) {
        *self.counts.entry(fingerprint).or_insert(0) += 1;
    }

    fn release(&mut self, fingerprint: &TemplateFingerprint) {
        let Some(count) = self.counts.get_mut(fingerprint) else {
            return;
        };
        *count -= 1;
        if *count == 0 {
            self.counts.remove(fingerprint);
        }
    }

    fn is_pinned(&self, fingerprint: &TemplateFingerprint) -> bool {
        self.counts.get(fingerprint).is_some_and(|count| *count > 0)
    }
}

impl TemplateStore {
    /// Create or initialize a store at `root`.
    ///
    /// The parent directory of `root` must already exist. Internal
    /// `by-fingerprint`, `staging`, and `index.json` entries are created by
    /// this explicit initialization call.
    pub fn create(root: impl Into<PathBuf>, capacity: usize) -> Result<Self, TemplateStoreError> {
        ensure_capacity(capacity)?;
        let layout = StoreLayout::new(root.into());
        layout.ensure_new_store_dirs()?;
        let index = layout.index_path();
        if !index.exists() {
            Index::empty().write(&index)?;
        } else {
            Index::read(&index)?;
        }
        Ok(Self {
            layout,
            capacity,
            pins: Arc::new(Mutex::new(PinState::default())),
            sequence: AtomicU64::new(0),
        })
    }

    /// Open an already-initialized store.
    pub fn open(root: impl Into<PathBuf>, capacity: usize) -> Result<Self, TemplateStoreError> {
        ensure_capacity(capacity)?;
        let layout = StoreLayout::new(root.into());
        layout.ensure_existing_store_dirs()?;
        Index::read(&layout.index_path())?;
        Ok(Self {
            layout,
            capacity,
            pins: Arc::new(Mutex::new(PinState::default())),
            sequence: AtomicU64::new(0),
        })
    }

    /// Return the store root path.
    #[must_use]
    pub fn root(&self) -> &Path {
        self.layout.root()
    }

    /// Return the visible content-addressed directory for `fingerprint`.
    #[must_use]
    pub fn template_dir(&self, fingerprint: &TemplateFingerprint) -> PathBuf {
        self.layout.template_dir(fingerprint)
    }

    /// List template index entries.
    pub fn list(&self) -> Result<Vec<TemplateSummary>, TemplateStoreError> {
        let index = self.read_index()?;
        Ok(index
            .entries
            .into_iter()
            .map(|entry| TemplateSummary {
                fingerprint: entry.fingerprint,
                last_used_unix_ms: entry.last_used_unix_ms,
                size_bytes: entry.size_bytes,
            })
            .collect())
    }

    /// Return template fingerprints whose manifest references `image_digest`.
    pub fn templates_referencing_image(
        &self,
        image_digest: &crate::ImageDigest,
    ) -> Result<Vec<TemplateFingerprint>, TemplateStoreError> {
        let index = self.read_index()?;
        let mut fingerprints = Vec::new();
        for entry in index.entries {
            let body_paths = self
                .layout
                .body_paths(&self.layout.template_dir(&entry.fingerprint));
            let manifest = TemplateManifest::read(&body_paths.manifest)?;
            if manifest
                .inputs
                .pmem_image_digest_set()
                .iter()
                .any(|pmem| pmem.image_digest() == image_digest)
            {
                fingerprints.push(entry.fingerprint);
            }
        }
        fingerprints.sort_by_key(TemplateFingerprint::to_hex);
        Ok(fingerprints)
    }

    /// Read a committed template manifest by fingerprint.
    pub fn manifest(
        &self,
        fingerprint: &TemplateFingerprint,
    ) -> Result<TemplateManifest, TemplateStoreError> {
        let dir = self.layout.template_dir(fingerprint);
        if !dir.is_dir() {
            return Err(TemplateStoreError::TemplateMissing {
                fingerprint: *fingerprint,
            });
        }
        let body_paths = self.layout.body_paths(&dir);
        TemplateManifest::read(&body_paths.manifest)
    }

    /// Remove one unpinned committed template.
    pub fn remove(&self, fingerprint: &TemplateFingerprint) -> Result<(), TemplateStoreError> {
        if self.is_pinned(fingerprint) {
            return Err(TemplateStoreError::TemplatePinned {
                fingerprint: *fingerprint,
            });
        }
        let dir = self.layout.template_dir(fingerprint);
        if !dir.is_dir() {
            return Err(TemplateStoreError::TemplateMissing {
                fingerprint: *fingerprint,
            });
        }
        self.remove_template_dir(fingerprint)?;
        let mut index = self.read_index()?;
        index.remove(fingerprint);
        index.write(&self.layout.index_path())
    }

    /// Look up live inputs and return a pinned template on cache hit.
    pub fn lookup(
        &self,
        live_inputs: &TemplateInputs,
    ) -> Result<Option<PinnedTemplate>, TemplateStoreError> {
        let fingerprint = TemplateFingerprint::compute(live_inputs);
        if !self.layout.template_dir(&fingerprint).exists() {
            return Ok(None);
        }
        self.pin(&fingerprint, live_inputs).map(Some)
    }

    /// Reserve a staging directory for a cache miss.
    pub fn reserve(
        &self,
        inputs: TemplateInputs,
        restore_layout: TemplateRestoreLayout,
    ) -> Result<TemplateBuildPlan, TemplateStoreError> {
        let fingerprint = TemplateFingerprint::compute(&inputs);
        let visible_dir = self.layout.template_dir(&fingerprint);
        if visible_dir.exists() {
            return Err(TemplateStoreError::TemplateExists { fingerprint });
        }
        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst);
        let staging_dir = self.layout.staging_dir(&fingerprint, sequence);
        std::fs::create_dir(&staging_dir).map_err(wrap_io(&staging_dir))?;
        let body_paths = self.layout.body_paths(&staging_dir);
        Ok(TemplateBuildPlan {
            fingerprint,
            staging_dir,
            body_paths,
            inputs,
            restore_layout,
        })
    }

    /// Commit a complete staging body into the content-addressed store.
    pub fn commit(&self, plan: TemplateBuildPlan) -> Result<PinnedTemplate, TemplateStoreError> {
        let visible_dir = self.layout.template_dir(&plan.fingerprint);
        if visible_dir.exists() {
            return Err(TemplateStoreError::TemplateExists {
                fingerprint: plan.fingerprint,
            });
        }
        ensure_body_file(&plan.body_paths.vm_state)?;
        ensure_body_file(&plan.body_paths.mem)?;

        let visible_paths = self.layout.body_paths(&visible_dir);
        let snapshot_manifest = build_snapshot_manifest(
            &plan.body_paths.snapshot_paths(),
            &visible_paths.snapshot_paths(),
            plan.inputs.firecracker_version(),
        )?;
        let manifest = TemplateManifest {
            fingerprint: plan.fingerprint,
            inputs: plan.inputs.clone(),
            restore_layout: plan.restore_layout.clone(),
            schema_version: SCHEMA_VERSION,
            snapshot_manifest,
        };
        validate_manifest_identity(&manifest, &plan.fingerprint, &plan.inputs)?;

        let tmp_manifest = plan.body_paths.manifest.with_extension(format!(
            "json.tmp.{}",
            self.sequence.fetch_add(1, Ordering::SeqCst)
        ));
        manifest.write(&tmp_manifest)?;
        std::fs::rename(&tmp_manifest, &plan.body_paths.manifest)
            .map_err(wrap_io(&plan.body_paths.manifest))?;
        let snapshot_manifest_path = plan.staging_dir.join(SNAPSHOT_MANIFEST_FILE);
        let tmp_snapshot_manifest = snapshot_manifest_path.with_extension(format!(
            "json.tmp.{}",
            self.sequence.fetch_add(1, Ordering::SeqCst)
        ));
        crate::index::write_pretty_json(&manifest.snapshot_manifest, &tmp_snapshot_manifest)?;
        std::fs::rename(&tmp_snapshot_manifest, &snapshot_manifest_path)
            .map_err(wrap_io(&snapshot_manifest_path))?;

        std::fs::rename(&plan.staging_dir, &visible_dir).map_err(wrap_io(&visible_dir))?;
        let size_bytes = template_size_bytes(&visible_paths)?;
        self.update_index(IndexEntry {
            fingerprint: plan.fingerprint,
            last_used_unix_ms: unix_time_ms()?,
            size_bytes,
        })?;
        let pinned = self.pin(&plan.fingerprint, &plan.inputs)?;
        self.evict_lru_to_capacity()?;
        Ok(pinned)
    }

    /// Pin an existing template by fingerprint and validate it against live inputs.
    pub fn pin(
        &self,
        fingerprint: &TemplateFingerprint,
        live_inputs: &TemplateInputs,
    ) -> Result<PinnedTemplate, TemplateStoreError> {
        let dir = self.layout.template_dir(fingerprint);
        if !dir.is_dir() {
            return Err(TemplateStoreError::TemplateMissing {
                fingerprint: *fingerprint,
            });
        }
        let body_paths = self.layout.body_paths(&dir);
        let manifest = TemplateManifest::read(&body_paths.manifest)?;
        validate_manifest_identity(&manifest, fingerprint, live_inputs)?;
        ensure_body_file(&body_paths.vm_state)?;
        ensure_body_file(&body_paths.mem)?;

        let pin = self.acquire_pin(*fingerprint);
        self.update_index(IndexEntry {
            fingerprint: *fingerprint,
            last_used_unix_ms: unix_time_ms()?,
            size_bytes: template_size_bytes(&body_paths)?,
        })?;
        Ok(PinnedTemplate {
            reference: TemplateRef::new(*fingerprint),
            body_paths,
            manifest,
            _pin: pin,
        })
    }

    /// Remove unpinned templates whose manifest does not match `live_inputs`.
    pub fn evict_invalidated(
        &self,
        live_inputs: &TemplateInputs,
    ) -> Result<usize, TemplateStoreError> {
        let live = TemplateFingerprint::compute(live_inputs);
        let mut index = self.read_index()?;
        let mut removed = 0;
        let entries = index.entries.clone();
        for entry in entries {
            if entry.fingerprint == live {
                continue;
            }
            if self.is_pinned(&entry.fingerprint) {
                continue;
            }
            self.remove_template_dir(&entry.fingerprint)?;
            index.remove(&entry.fingerprint);
            removed += 1;
        }
        index.write(&self.layout.index_path())?;
        Ok(removed)
    }

    /// Remove unpinned templates in the same conservative prune scope.
    ///
    /// Scope is the subset of manifest inputs that identify the template
    /// family without depending on host kernel, Firecracker, or guest kernel
    /// version: pmem image set, post-init state digest, and hook set. This
    /// lets callers prune host/FC/kernel-invalidated templates without sweeping
    /// unrelated families that happen to share the same store.
    pub fn evict_invalidated_in_scope(
        &self,
        live_inputs: &TemplateInputs,
    ) -> Result<usize, TemplateStoreError> {
        let live = TemplateFingerprint::compute(live_inputs);
        let mut index = self.read_index()?;
        let mut removed = 0;
        let entries = index.entries.clone();
        for entry in entries {
            if entry.fingerprint == live {
                continue;
            }
            if self.is_pinned(&entry.fingerprint) {
                continue;
            }
            let body_paths = self
                .layout
                .body_paths(&self.layout.template_dir(&entry.fingerprint));
            let manifest = TemplateManifest::read(&body_paths.manifest)?;
            if !same_prune_scope(&manifest.inputs, live_inputs) {
                continue;
            }
            self.remove_template_dir(&entry.fingerprint)?;
            index.remove(&entry.fingerprint);
            removed += 1;
        }
        index.write(&self.layout.index_path())?;
        Ok(removed)
    }

    /// Evict least-recently-used unpinned templates until the store is within capacity.
    pub fn evict_lru_to_capacity(&self) -> Result<usize, TemplateStoreError> {
        let mut index = self.read_index()?;
        let mut removed = 0;
        while index.entries.len() > self.capacity {
            let Some((position, entry)) = index
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| !self.is_pinned(&entry.fingerprint))
                .min_by_key(|(_, entry)| entry.last_used_unix_ms)
            else {
                return Err(TemplateStoreError::AllCandidatesPinned);
            };
            let fingerprint = entry.fingerprint;
            self.remove_template_dir(&fingerprint)?;
            index.entries.remove(position);
            removed += 1;
        }
        index.write(&self.layout.index_path())?;
        Ok(removed)
    }

    fn acquire_pin(&self, fingerprint: TemplateFingerprint) -> TemplatePin {
        self.pins
            .lock()
            .expect("template pin mutex poisoned")
            .acquire(fingerprint);
        TemplatePin {
            fingerprint,
            pins: Arc::clone(&self.pins),
        }
    }

    fn is_pinned(&self, fingerprint: &TemplateFingerprint) -> bool {
        self.pins
            .lock()
            .expect("template pin mutex poisoned")
            .is_pinned(fingerprint)
    }

    fn read_index(&self) -> Result<Index, TemplateStoreError> {
        Index::read(&self.layout.index_path())
    }

    fn update_index(&self, entry: IndexEntry) -> Result<(), TemplateStoreError> {
        let mut index = self.read_index()?;
        index.upsert(entry);
        index.write(&self.layout.index_path())
    }

    fn remove_template_dir(
        &self,
        fingerprint: &TemplateFingerprint,
    ) -> Result<(), TemplateStoreError> {
        let dir = self.layout.template_dir(fingerprint);
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => Ok(()),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(TemplateStoreError::Io { path: dir, source }),
        }
    }
}

fn same_prune_scope(candidate: &TemplateInputs, live: &TemplateInputs) -> bool {
    candidate.pmem_image_digest_set() == live.pmem_image_digest_set()
        && candidate.post_init_state_digest() == live.post_init_state_digest()
        && candidate.hook_spec_set() == live.hook_spec_set()
}

fn ensure_capacity(capacity: usize) -> Result<(), TemplateStoreError> {
    if capacity == 0 {
        return Err(TemplateStoreError::CapacityZero);
    }
    Ok(())
}

fn ensure_body_file(path: &Path) -> Result<(), TemplateStoreError> {
    if path.is_file() {
        return Ok(());
    }
    Err(TemplateStoreError::MissingBody {
        path: path.to_path_buf(),
    })
}

fn validate_manifest_identity(
    manifest: &TemplateManifest,
    path_fingerprint: &TemplateFingerprint,
    live_inputs: &TemplateInputs,
) -> Result<(), TemplateStoreError> {
    if &manifest.fingerprint != path_fingerprint {
        return Err(TemplateStoreError::FingerprintMismatch {
            stored: *path_fingerprint,
            live: manifest.fingerprint,
        });
    }
    let stored_from_inputs = TemplateFingerprint::compute(&manifest.inputs);
    if manifest.fingerprint != stored_from_inputs {
        return Err(TemplateStoreError::FingerprintMismatch {
            stored: manifest.fingerprint,
            live: stored_from_inputs,
        });
    }
    let live = TemplateFingerprint::compute(live_inputs);
    if manifest.fingerprint != live {
        return Err(TemplateStoreError::FingerprintMismatch {
            stored: manifest.fingerprint,
            live,
        });
    }
    if manifest.snapshot_manifest.schema_version != m80_snapshot::SCHEMA_VERSION {
        return Err(TemplateStoreError::InvalidValue {
            field: "template.snapshot_manifest.schema_version",
            reason: "must match active m80-snapshot schema",
        });
    }
    if manifest.snapshot_manifest.expected_firecracker_version
        != manifest.inputs.firecracker_version()
    {
        return Err(TemplateStoreError::InvalidValue {
            field: "template.snapshot_manifest.expected_firecracker_version",
            reason: "must match template inputs Firecracker version",
        });
    }
    Ok(())
}

fn build_snapshot_manifest(
    read_paths: &SnapshotPaths,
    recorded_paths: &SnapshotPaths,
    expected_firecracker_version: &str,
) -> Result<SnapshotManifest, TemplateStoreError> {
    let artifacts = vec![
        artifact_for_path(ArtifactKind::Memory, &read_paths.mem, &recorded_paths.mem)?,
        artifact_for_path(
            ArtifactKind::VmState,
            &read_paths.vm_state,
            &recorded_paths.vm_state,
        )?,
    ];
    Ok(SnapshotManifest {
        artifact_set_sha256: hex::encode(artifact_set_sha256(&artifacts)),
        artifacts,
        created_at_unix_ms: unix_time_ms()?,
        expected_firecracker_version: expected_firecracker_version.to_owned(),
        schema_version: m80_snapshot::SCHEMA_VERSION,
    })
}

fn artifact_for_path(
    kind: ArtifactKind,
    read_path: &Path,
    recorded_path: &Path,
) -> Result<Artifact, TemplateStoreError> {
    let bytes = std::fs::read(read_path).map_err(wrap_io(read_path))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(Artifact {
        kind,
        path: recorded_path.to_path_buf(),
        sha256: hex::encode(hasher.finalize()),
        size: bytes.len() as u64,
    })
}

fn artifact_set_sha256(artifacts: &[Artifact]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for artifact in artifacts {
        let bytes = serde_json::to_vec(artifact).expect("Artifact serialization is infallible");
        hasher.update(&bytes);
    }
    hasher.finalize().into()
}

fn template_size_bytes(paths: &TemplateBodyPaths) -> Result<u64, TemplateStoreError> {
    let mut total = 0;
    let snapshot_manifest = paths
        .vm_state
        .parent()
        .expect("template body path has parent")
        .join(SNAPSHOT_MANIFEST_FILE);
    for path in [
        &paths.vm_state,
        &paths.mem,
        &paths.manifest,
        &snapshot_manifest,
    ] {
        total += std::fs::metadata(path).map_err(wrap_io(path))?.len();
    }
    Ok(total)
}

fn unix_time_ms() -> Result<u64, TemplateStoreError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|source| TemplateStoreError::InvalidValue {
            field: "template.clock",
            reason: if source.duration().is_zero() {
                "system clock is before unix epoch"
            } else {
                "system clock error"
            },
        })?;
    Ok(elapsed.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HookSpecSet, JailBackingPath, TemplateDigest};

    #[test]
    fn manifest_identity_rejects_snapshot_manifest_version_mismatch() {
        let live = inputs("v1.15.1");
        let mut manifest = manifest_for(live.clone());
        manifest.snapshot_manifest.expected_firecracker_version = "v9.99.0".to_owned();

        let err = validate_manifest_identity(&manifest, &manifest.fingerprint, &live)
            .expect_err("snapshot manifest version must match template inputs");

        assert!(
            matches!(
                err,
                TemplateStoreError::InvalidValue {
                    field: "template.snapshot_manifest.expected_firecracker_version",
                    ..
                }
            ),
            "expected snapshot manifest version rejection, got {err:?}"
        );
    }

    #[test]
    fn manifest_identity_rejects_snapshot_schema_mismatch() {
        let live = inputs("v1.15.1");
        let mut manifest = manifest_for(live.clone());
        manifest.snapshot_manifest.schema_version = m80_snapshot::SCHEMA_VERSION + 1;

        let err = validate_manifest_identity(&manifest, &manifest.fingerprint, &live)
            .expect_err("snapshot manifest schema must match active snapshot schema");

        assert!(
            matches!(
                err,
                TemplateStoreError::InvalidValue {
                    field: "template.snapshot_manifest.schema_version",
                    ..
                }
            ),
            "expected snapshot schema rejection, got {err:?}"
        );
    }

    fn manifest_for(inputs: TemplateInputs) -> TemplateManifest {
        let fingerprint = TemplateFingerprint::compute(&inputs);
        TemplateManifest {
            fingerprint,
            inputs,
            restore_layout: TemplateRestoreLayout::new(
                JailBackingPath::parse("/snapshot/vm.snap").expect("vm path"),
                JailBackingPath::parse("/snapshot/mem.snap").expect("mem path"),
                Vec::new(),
            ),
            schema_version: SCHEMA_VERSION,
            snapshot_manifest: SnapshotManifest {
                artifact_set_sha256: "0".repeat(64),
                artifacts: Vec::new(),
                created_at_unix_ms: 0,
                expected_firecracker_version: "v1.15.1".to_owned(),
                schema_version: m80_snapshot::SCHEMA_VERSION,
            },
        }
    }

    fn inputs(firecracker_version: &str) -> TemplateInputs {
        TemplateInputs::new(
            "host-kernel".to_owned(),
            firecracker_version.to_owned(),
            digest('1'),
            Vec::new(),
            digest('2'),
            HookSpecSet::empty(),
        )
        .expect("template inputs")
    }

    fn digest(ch: char) -> TemplateDigest {
        TemplateDigest::parse(&ch.to_string().repeat(64)).expect("digest")
    }
}
