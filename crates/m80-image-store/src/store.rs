//! Store layout and import/resolve operations.

use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::os::unix::fs::{FileTypeExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};

use nix::fcntl::OFlag;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::build::build_image;
use crate::lock::{StoreLock, TEMPLATE_COORDINATION_LOCK_FILE_NAME};
use crate::{
    ErofsImage, Ext4Image, ImageArtifact, ImageDigest, ImageKind, StoreError, DEFAULT_STORE_ROOT,
};

const METADATA_SCHEMA_VERSION: u32 = 1;
const METADATA_FILE_NAME: &str = "metadata.json";
const SHARED_REFS_DIR_NAME: &str = "shared";
const REFS_DIR_NAME: &str = "refs";

/// Content-addressed image artifact store.
#[derive(Debug, Clone)]
pub struct ImageStore {
    root: PathBuf,
}

/// One artifact entry recorded in the image store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRecord {
    digest: ImageDigest,
    kind: ImageKind,
    path: PathBuf,
    size_bytes: u64,
}

impl ImageRecord {
    /// Return the artifact digest.
    #[must_use]
    pub fn digest(&self) -> &ImageDigest {
        &self.digest
    }

    /// Return the artifact kind.
    #[must_use]
    pub fn kind(&self) -> ImageKind {
        self.kind
    }

    /// Return the canonical artifact path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return the artifact size in bytes.
    #[must_use]
    pub fn size_bytes(&self) -> u64 {
        self.size_bytes
    }
}

/// Active-use marker for one VM using a shared image-store artifact.
#[derive(Debug)]
pub struct SharedImageRef {
    root: PathBuf,
    digest: ImageDigest,
    vm_id: String,
}

/// Process-local guard for image/template-store coordination.
///
/// Template builds hold this lock shared while committing a template that
/// references image-store artifacts. Executable image GC holds it exclusive
/// while scanning committed template refs and deleting candidates.
pub struct ImageTemplateCoordinationGuard {
    _lock: StoreLock,
}

impl SharedImageRef {
    /// Return the digest this marker references.
    #[must_use]
    pub fn digest(&self) -> &ImageDigest {
        &self.digest
    }

    /// Return the VM id that owns this marker.
    #[must_use]
    pub fn vm_id(&self) -> &str {
        &self.vm_id
    }

    /// Return the on-disk marker path.
    #[must_use]
    pub fn marker_path(&self) -> PathBuf {
        shared_ref_marker_path(&self.root, &self.digest, &self.vm_id)
    }

    /// Remove the active-use marker.
    pub fn release(self) -> Result<(), StoreError> {
        release_shared_ref_marker(&self.root, &self.digest, &self.vm_id)?;
        Ok(())
    }
}

impl ImageStore {
    /// Open an existing image store root.
    ///
    /// The root must be absolute, must already exist, must be a directory, and
    /// must not itself be a symlink. Use [`DEFAULT_STORE_ROOT`] for the
    /// production path.
    pub fn open(root: &Path) -> Result<Self, StoreError> {
        if !root.is_absolute() {
            return Err(StoreError::InvalidPath {
                path: root.to_path_buf(),
                reason: "store root must be absolute",
            });
        }
        let metadata = std::fs::symlink_metadata(root).map_err(|source| StoreError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            return Err(StoreError::InvalidPath {
                path: root.to_path_buf(),
                reason: "store root must not be a symlink",
            });
        }
        if !file_type.is_dir() {
            return Err(StoreError::InvalidPath {
                path: root.to_path_buf(),
                reason: "store root must be a directory",
            });
        }
        let canonical = root.canonicalize().map_err(|source| StoreError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        Ok(Self { root: canonical })
    }

    /// Open the default production image store root.
    pub fn open_default() -> Result<Self, StoreError> {
        Self::open(Path::new(DEFAULT_STORE_ROOT))
    }

    /// Return the canonical store root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Hold the image/template coordination lock in shared mode.
    ///
    /// Use this around template build/commit windows that may publish manifests
    /// referencing image-store artifacts.
    pub fn acquire_template_build_guard(
        &self,
    ) -> Result<ImageTemplateCoordinationGuard, StoreError> {
        Ok(ImageTemplateCoordinationGuard {
            _lock: StoreLock::shared_named(&self.root, TEMPLATE_COORDINATION_LOCK_FILE_NAME)?,
        })
    }

    /// Hold the image/template coordination lock in exclusive mode.
    ///
    /// Use this around executable image GC planning plus deletion. It prevents
    /// a concurrent template build from committing a new image reference
    /// between the GC's template scan and remove calls.
    pub fn acquire_gc_execute_guard(&self) -> Result<ImageTemplateCoordinationGuard, StoreError> {
        Ok(ImageTemplateCoordinationGuard {
            _lock: StoreLock::exclusive_named(&self.root, TEMPLATE_COORDINATION_LOCK_FILE_NAME)?,
        })
    }

    /// Ingest a pre-built artifact and return its content digest.
    ///
    /// The source is hashed first, then copied into
    /// `<root>/<digest[0..2]>/<digest>/{image.erofs|image.ext4}` under an
    /// exclusive store lock.
    pub fn import_existing(
        &self,
        source: &Path,
        kind: ImageKind,
    ) -> Result<ImageDigest, StoreError> {
        reject_symlink(source)?;
        if kind == ImageKind::Erofs {
            crate::erofs::validate_supported_erofs(source)?;
        }
        let digest = hash_file(source)?;
        let _lock = StoreLock::exclusive(&self.root)?;
        let entry = self.entry_paths(&digest);
        ensure_entry_dirs(&entry)?;
        let dest = entry.artifact_path(kind);
        install_artifact(source, &dest, &digest)?;
        let size_bytes = dest
            .metadata()
            .map_err(|source| StoreError::Io {
                path: dest.clone(),
                source,
            })?
            .len();
        write_metadata_entry(&entry.metadata, kind, &digest, size_bytes)?;
        Ok(digest)
    }

    /// Build a tiny local-dev/test artifact and import it into the store.
    ///
    /// This wraps host `mkfs.erofs` / `mkfs.ext4` for tests and operator
    /// experiments only. Production image pipelines live outside m80 and should
    /// feed their completed artifacts through [`Self::import_existing`].
    pub fn build_minimal_test_image(
        &self,
        source_dir: &Path,
        kind: ImageKind,
    ) -> Result<ImageDigest, StoreError> {
        reject_symlink(source_dir)?;
        if !source_dir.is_dir() {
            return Err(StoreError::InvalidPath {
                path: source_dir.to_path_buf(),
                reason: "source_dir must be a directory",
            });
        }
        let temp = self.root.join(format!(
            ".m80-image-build-{}-{}.tmp",
            std::process::id(),
            kind.as_str()
        ));
        let _ = std::fs::remove_file(&temp);
        build_image(source_dir, &temp, kind)?;
        let result = self.import_existing(&temp, kind);
        let _ = std::fs::remove_file(&temp);
        result
    }

    /// List every artifact recorded in the store.
    pub fn list(&self) -> Result<Vec<ImageRecord>, StoreError> {
        let _lock = StoreLock::shared(&self.root)?;
        let mut records = Vec::new();
        for shard in read_store_shards(&self.root)? {
            for digest_dir in read_digest_dirs(&shard)? {
                let digest_name = digest_dir
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| StoreError::InvalidMetadata {
                        path: digest_dir.clone(),
                        reason: "digest directory name must be UTF-8",
                    })?;
                let digest =
                    ImageDigest::parse(digest_name).map_err(|_| StoreError::InvalidMetadata {
                        path: digest_dir.clone(),
                        reason: "digest directory name must be a lowercase sha256 digest",
                    })?;
                let entry = self.entry_paths(&digest);
                records.extend(self.records_for_entry(&entry, &digest)?);
            }
        }
        records.sort_by(|left, right| {
            left.digest
                .as_str()
                .cmp(right.digest.as_str())
                .then_with(|| left.kind.as_str().cmp(right.kind.as_str()))
        });
        Ok(records)
    }

    /// Describe every artifact kind stored for one digest.
    pub fn describe(&self, digest: &ImageDigest) -> Result<Vec<ImageRecord>, StoreError> {
        let _lock = StoreLock::shared(&self.root)?;
        let entry = self.entry_paths(digest);
        let records = self.records_for_entry(&entry, digest)?;
        if records.is_empty() {
            return Err(StoreError::NotFound {
                digest: digest.clone(),
            });
        }
        Ok(records)
    }

    /// Resolve a digest to its stored artifact.
    ///
    /// Resolution takes a shared store lock and verifies the final path is a
    /// regular file opened with `O_NOFOLLOW`.
    pub fn resolve(&self, digest: &ImageDigest) -> Result<ImageArtifact, StoreError> {
        let _lock = StoreLock::shared(&self.root)?;
        let entry = self.entry_paths(digest);
        let metadata = read_metadata(&entry.metadata, digest)?;
        let [artifact] = metadata.artifacts.as_slice() else {
            return Err(StoreError::AmbiguousDigest {
                digest: digest.clone(),
            });
        };
        self.resolve_kind(digest, artifact.kind, artifact.size_bytes)
    }

    /// Resolve a digest to a specific stored artifact kind.
    pub fn resolve_as(
        &self,
        digest: &ImageDigest,
        kind: ImageKind,
    ) -> Result<ImageArtifact, StoreError> {
        let _lock = StoreLock::shared(&self.root)?;
        let entry = self.entry_paths(digest);
        let metadata = read_metadata(&entry.metadata, digest)?;
        let artifact = metadata
            .artifacts
            .iter()
            .find(|artifact| artifact.kind == kind)
            .ok_or_else(|| StoreError::NotFound {
                digest: digest.clone(),
            })?;
        self.resolve_kind(digest, kind, artifact.size_bytes)
    }

    /// Record that `vm_id` is actively using a shared image artifact.
    ///
    /// The marker is stored under `<store>/shared/<digest>/refs/<vm_id>`.
    /// This does not make the content-addressed artifact temporary; artifacts
    /// remain operator-managed store inputs. The marker only tracks active VM
    /// use so startup recovery can sweep stale references without deleting the
    /// canonical image.
    pub fn acquire_shared_ref(
        &self,
        digest: &ImageDigest,
        vm_id: &str,
    ) -> Result<SharedImageRef, StoreError> {
        validate_vm_marker_name(vm_id)?;
        let _lock = StoreLock::exclusive(&self.root)?;
        let entry = self.entry_paths(digest);
        read_metadata(&entry.metadata, digest)?;
        let refs_dir = shared_refs_dir(&self.root, digest);
        std::fs::create_dir_all(&refs_dir).map_err(|source| StoreError::Io {
            path: refs_dir.clone(),
            source,
        })?;
        let marker = shared_ref_marker_path(&self.root, digest, vm_id);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&marker)
            .map_err(|source| {
                if source.kind() == std::io::ErrorKind::AlreadyExists {
                    StoreError::SharedRefAlreadyExists {
                        digest: digest.clone(),
                        vm_id: vm_id.to_owned(),
                    }
                } else {
                    StoreError::Io {
                        path: marker.clone(),
                        source,
                    }
                }
            })?;
        file.write_all(vm_id.as_bytes())
            .and_then(|()| file.write_all(b"\n"))
            .map_err(|source| StoreError::Io {
                path: marker.clone(),
                source,
            })?;
        file.sync_all().map_err(|source| StoreError::Io {
            path: marker,
            source,
        })?;
        Ok(SharedImageRef {
            root: self.root.clone(),
            digest: digest.clone(),
            vm_id: vm_id.to_owned(),
        })
    }

    /// Count active-use markers for a shared image artifact.
    pub fn shared_ref_count(&self, digest: &ImageDigest) -> Result<usize, StoreError> {
        let _lock = StoreLock::shared(&self.root)?;
        count_shared_refs(&self.root, digest)
    }

    /// Remove shared-image markers whose VM id is not in `live_vm_ids`.
    ///
    /// This is startup recovery for process-crash residue. It never deletes
    /// content-addressed image artifacts.
    pub fn sweep_shared_refs<I, S>(&self, live_vm_ids: I) -> Result<usize, StoreError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let live = live_vm_ids
            .into_iter()
            .map(|vm_id| vm_id.as_ref().to_owned())
            .collect::<BTreeSet<_>>();
        let _lock = StoreLock::exclusive(&self.root)?;
        let shared_root = self.root.join(SHARED_REFS_DIR_NAME);
        let entries = match std::fs::read_dir(&shared_root) {
            Ok(entries) => entries,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(source) => {
                return Err(StoreError::Io {
                    path: shared_root,
                    source,
                });
            }
        };
        let mut removed = 0;
        for entry in entries {
            let entry = entry.map_err(|source| StoreError::Io {
                path: shared_root.clone(),
                source,
            })?;
            let digest_dir = entry.path();
            if !digest_dir.is_dir() {
                continue;
            }
            let refs_dir = digest_dir.join(REFS_DIR_NAME);
            let refs = match std::fs::read_dir(&refs_dir) {
                Ok(refs) => refs,
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => continue,
                Err(source) => {
                    return Err(StoreError::Io {
                        path: refs_dir,
                        source,
                    });
                }
            };
            for marker in refs {
                let marker = marker.map_err(|source| StoreError::Io {
                    path: refs_dir.clone(),
                    source,
                })?;
                let marker_path = marker.path();
                if !marker_path.is_file() {
                    continue;
                }
                let Some(vm_id) = marker.file_name().to_str().map(str::to_owned) else {
                    return Err(StoreError::InvalidPath {
                        path: marker_path,
                        reason: "shared ref marker name must be UTF-8",
                    });
                };
                validate_vm_marker_name(&vm_id)?;
                if live.contains(&vm_id) {
                    continue;
                }
                std::fs::remove_file(&marker_path).map_err(|source| StoreError::Io {
                    path: marker_path,
                    source,
                })?;
                removed += 1;
            }
            remove_empty_dir(&refs_dir)?;
            remove_empty_dir(&digest_dir)?;
        }
        remove_empty_dir(&shared_root)?;
        Ok(removed)
    }

    /// Verify that stored bytes still match the requested digest.
    pub fn verify(&self, digest: &ImageDigest) -> Result<(), StoreError> {
        let _lock = StoreLock::shared(&self.root)?;
        let entry = self.entry_paths(digest);
        let metadata = read_metadata(&entry.metadata, digest)?;
        for artifact in metadata.artifacts.iter() {
            let path = entry.artifact_path(artifact.kind);
            open_regular_nofollow(&path)?;
            let got = hash_file(&path)?;
            if &got != digest {
                return Err(StoreError::DigestMismatch {
                    expected: digest.clone(),
                    got,
                });
            }
        }
        Ok(())
    }

    /// Remove every artifact stored for one digest.
    ///
    /// Removal fails while Shared active-use markers exist. Template-reference
    /// checks live above this crate because the image store does not own the
    /// snapshot-template schema.
    pub fn remove(&self, digest: &ImageDigest) -> Result<Vec<ImageRecord>, StoreError> {
        let _lock = StoreLock::exclusive(&self.root)?;
        let ref_count = count_shared_refs(&self.root, digest)?;
        if ref_count > 0 {
            return Err(StoreError::ImageInUse {
                digest: digest.clone(),
                ref_count,
            });
        }
        let entry = self.entry_paths(digest);
        let records = self.records_for_entry(&entry, digest)?;
        if records.is_empty() {
            return Err(StoreError::NotFound {
                digest: digest.clone(),
            });
        }
        for record in &records {
            std::fs::remove_file(record.path()).map_err(|source| StoreError::Io {
                path: record.path().to_path_buf(),
                source,
            })?;
        }
        std::fs::remove_file(&entry.metadata).map_err(|source| StoreError::Io {
            path: entry.metadata.clone(),
            source,
        })?;
        remove_empty_dir(&entry.dir)?;
        if let Some(shard) = entry.dir.parent() {
            remove_empty_dir(shard)?;
        }
        Ok(records)
    }

    fn resolve_kind(
        &self,
        digest: &ImageDigest,
        kind: ImageKind,
        size_bytes: u64,
    ) -> Result<ImageArtifact, StoreError> {
        let entry = self.entry_paths(digest);
        let path = entry.artifact_path(kind);
        open_regular_nofollow(&path)?;
        let canonical = path.canonicalize().map_err(|source| StoreError::Io {
            path: path.clone(),
            source,
        })?;
        match kind {
            ImageKind::Erofs => Ok(ImageArtifact::Erofs(ErofsImage::new(
                canonical,
                size_bytes,
                digest.clone(),
            ))),
            ImageKind::Ext4 => Ok(ImageArtifact::Ext4(Ext4Image::new(
                canonical,
                size_bytes,
                digest.clone(),
            ))),
        }
    }

    fn records_for_entry(
        &self,
        entry: &EntryPaths,
        digest: &ImageDigest,
    ) -> Result<Vec<ImageRecord>, StoreError> {
        let metadata = read_metadata(&entry.metadata, digest)?;
        let mut records = Vec::with_capacity(metadata.artifacts.len());
        for artifact in metadata.artifacts {
            let path = entry.artifact_path(artifact.kind);
            open_regular_nofollow(&path)?;
            let canonical = path.canonicalize().map_err(|source| StoreError::Io {
                path: path.clone(),
                source,
            })?;
            records.push(ImageRecord {
                digest: digest.clone(),
                kind: artifact.kind,
                path: canonical,
                size_bytes: artifact.size_bytes,
            });
        }
        records.sort_by_key(|record| record.kind.as_str());
        Ok(records)
    }

    fn entry_paths(&self, digest: &ImageDigest) -> EntryPaths {
        let s = digest.as_str();
        let dir = self.root.join(&s[..2]).join(s);
        EntryPaths {
            metadata: dir.join(METADATA_FILE_NAME),
            dir,
        }
    }
}

#[derive(Debug)]
struct EntryPaths {
    dir: PathBuf,
    metadata: PathBuf,
}

impl EntryPaths {
    fn artifact_path(&self, kind: ImageKind) -> PathBuf {
        self.dir.join(kind.file_name())
    }
}

fn read_store_shards(root: &Path) -> Result<Vec<PathBuf>, StoreError> {
    let entries = std::fs::read_dir(root).map_err(|source| StoreError::Io {
        path: root.to_path_buf(),
        source,
    })?;
    let mut shards = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| StoreError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Err(StoreError::InvalidMetadata {
                path,
                reason: "store entry name must be UTF-8",
            });
        };
        if name == crate::lock::LOCK_FILE_NAME
            || name == TEMPLATE_COORDINATION_LOCK_FILE_NAME
            || name == SHARED_REFS_DIR_NAME
        {
            continue;
        }
        if !path.is_dir() {
            return Err(StoreError::InvalidMetadata {
                path,
                reason: "store root entries must be shard directories",
            });
        }
        if name.len() != 2
            || !name
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(StoreError::InvalidMetadata {
                path,
                reason: "store shard directory must be two lowercase hex characters",
            });
        }
        shards.push(path);
    }
    shards.sort();
    Ok(shards)
}

fn read_digest_dirs(shard: &Path) -> Result<Vec<PathBuf>, StoreError> {
    let entries = std::fs::read_dir(shard).map_err(|source| StoreError::Io {
        path: shard.to_path_buf(),
        source,
    })?;
    let mut dirs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| StoreError::Io {
            path: shard.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if !path.is_dir() {
            return Err(StoreError::InvalidMetadata {
                path,
                reason: "store shard entries must be digest directories",
            });
        }
        dirs.push(path);
    }
    dirs.sort();
    Ok(dirs)
}

fn validate_vm_marker_name(vm_id: &str) -> Result<(), StoreError> {
    if !vm_id.is_empty()
        && vm_id != "."
        && vm_id != ".."
        && vm_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Ok(());
    }
    Err(StoreError::InvalidPath {
        path: PathBuf::from(vm_id),
        reason: "shared ref vm_id must be non-empty ASCII alphanumeric, '.', '_', or '-'",
    })
}

fn shared_refs_dir(root: &Path, digest: &ImageDigest) -> PathBuf {
    root.join(SHARED_REFS_DIR_NAME)
        .join(digest.as_str())
        .join(REFS_DIR_NAME)
}

fn shared_ref_marker_path(root: &Path, digest: &ImageDigest, vm_id: &str) -> PathBuf {
    shared_refs_dir(root, digest).join(vm_id)
}

fn release_shared_ref_marker(
    root: &Path,
    digest: &ImageDigest,
    vm_id: &str,
) -> Result<(), StoreError> {
    let _lock = StoreLock::exclusive(root)?;
    let marker = shared_ref_marker_path(root, digest, vm_id);
    match std::fs::remove_file(&marker) {
        Ok(()) => {}
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Err(StoreError::SharedRefNotFound {
                digest: digest.clone(),
                vm_id: vm_id.to_owned(),
            });
        }
        Err(source) => {
            return Err(StoreError::Io {
                path: marker,
                source,
            });
        }
    }
    let refs_dir = shared_refs_dir(root, digest);
    remove_empty_dir(&refs_dir)?;
    if let Some(digest_dir) = refs_dir.parent() {
        remove_empty_dir(digest_dir)?;
    }
    let shared_root = root.join(SHARED_REFS_DIR_NAME);
    remove_empty_dir(&shared_root)?;
    Ok(())
}

fn count_shared_refs(root: &Path, digest: &ImageDigest) -> Result<usize, StoreError> {
    let refs_dir = shared_refs_dir(root, digest);
    let refs = match std::fs::read_dir(&refs_dir) {
        Ok(refs) => refs,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(source) => {
            return Err(StoreError::Io {
                path: refs_dir,
                source,
            });
        }
    };
    let mut count = 0;
    for marker in refs {
        let marker = marker.map_err(|source| StoreError::Io {
            path: refs_dir.clone(),
            source,
        })?;
        if marker.path().is_file() {
            count += 1;
        }
    }
    Ok(count)
}

fn remove_empty_dir(path: &Path) -> Result<(), StoreError> {
    match std::fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(source)
            if source.kind() == std::io::ErrorKind::NotFound
                || source.raw_os_error() == Some(nix::libc::ENOTEMPTY) =>
        {
            Ok(())
        }
        Err(source) => Err(StoreError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoreMetadata {
    schema_version: u32,
    artifacts: Vec<ArtifactMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactMetadata {
    kind: ImageKind,
    digest: String,
    size_bytes: u64,
    file_name: String,
}

fn ensure_entry_dirs(entry: &EntryPaths) -> Result<(), StoreError> {
    let shard = entry.dir.parent().ok_or_else(|| StoreError::InvalidPath {
        path: entry.dir.clone(),
        reason: "entry dir must have a shard parent",
    })?;
    if !shard.exists() {
        std::fs::create_dir(shard).map_err(|source| StoreError::Io {
            path: shard.to_path_buf(),
            source,
        })?;
    }
    if !entry.dir.exists() {
        std::fs::create_dir(&entry.dir).map_err(|source| StoreError::Io {
            path: entry.dir.clone(),
            source,
        })?;
    }
    Ok(())
}

fn install_artifact(source: &Path, dest: &Path, digest: &ImageDigest) -> Result<(), StoreError> {
    if let Ok(metadata) = std::fs::symlink_metadata(dest) {
        if metadata.file_type().is_symlink() {
            return Err(StoreError::InvalidPath {
                path: dest.to_path_buf(),
                reason: "artifact path must not be a symlink",
            });
        }
        open_regular_nofollow(dest)?;
        let got = hash_file(dest)?;
        if &got != digest {
            return Err(StoreError::DigestMismatch {
                expected: digest.clone(),
                got,
            });
        }
        return Ok(());
    }

    let temp = dest.with_extension(format!("{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&temp);
    copy_nofollow(source, &temp)?;
    let got = hash_file(&temp)?;
    if &got != digest {
        let _ = std::fs::remove_file(&temp);
        return Err(StoreError::DigestMismatch {
            expected: digest.clone(),
            got,
        });
    }
    std::fs::rename(&temp, dest).map_err(|source| StoreError::Io {
        path: dest.to_path_buf(),
        source,
    })
}

fn copy_nofollow(source: &Path, dest: &Path) -> Result<(), StoreError> {
    let mut input = open_regular_nofollow(source)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(OFlag::O_NOFOLLOW.bits())
        .open(dest)
        .map_err(|source| StoreError::Io {
            path: dest.to_path_buf(),
            source,
        })?;
    std::io::copy(&mut input, &mut output).map_err(|source| StoreError::Io {
        path: dest.to_path_buf(),
        source,
    })?;
    output.sync_all().map_err(|source| StoreError::Io {
        path: dest.to_path_buf(),
        source,
    })
}

fn write_metadata_entry(
    path: &Path,
    kind: ImageKind,
    digest: &ImageDigest,
    size_bytes: u64,
) -> Result<(), StoreError> {
    let mut metadata = if path.exists() {
        read_metadata_file(path)?
    } else {
        StoreMetadata {
            schema_version: METADATA_SCHEMA_VERSION,
            artifacts: Vec::new(),
        }
    };
    metadata.artifacts.retain(|artifact| artifact.kind != kind);
    metadata.artifacts.push(ArtifactMetadata {
        kind,
        digest: digest.as_str().to_owned(),
        size_bytes,
        file_name: kind.file_name().to_owned(),
    });
    metadata
        .artifacts
        .sort_by_key(|artifact| artifact.kind.as_str());
    let bytes = serde_json::to_vec_pretty(&metadata).map_err(|source| StoreError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(OFlag::O_NOFOLLOW.bits())
        .open(&temp)
        .map_err(|source| StoreError::Io {
            path: temp.clone(),
            source,
        })?;
    file.write_all(&bytes).map_err(|source| StoreError::Io {
        path: temp.clone(),
        source,
    })?;
    file.write_all(b"\n").map_err(|source| StoreError::Io {
        path: temp.clone(),
        source,
    })?;
    file.sync_all().map_err(|source| StoreError::Io {
        path: temp.clone(),
        source,
    })?;
    drop(file);
    std::fs::rename(&temp, path).map_err(|source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn read_metadata(path: &Path, digest: &ImageDigest) -> Result<StoreMetadata, StoreError> {
    let metadata = read_metadata_file(path).map_err(|err| match err {
        StoreError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound => {
            StoreError::NotFound {
                digest: digest.clone(),
            }
        }
        other => other,
    })?;
    for artifact in &metadata.artifacts {
        if artifact.digest != digest.as_str() {
            return Err(StoreError::InvalidMetadata {
                path: path.to_path_buf(),
                reason: "artifact digest does not match metadata path",
            });
        }
        if artifact.file_name != artifact.kind.file_name() {
            return Err(StoreError::InvalidMetadata {
                path: path.to_path_buf(),
                reason: "artifact filename does not match kind",
            });
        }
    }
    if metadata.artifacts.is_empty() {
        return Err(StoreError::InvalidMetadata {
            path: path.to_path_buf(),
            reason: "metadata must contain at least one artifact",
        });
    }
    Ok(metadata)
}

fn read_metadata_file(path: &Path) -> Result<StoreMetadata, StoreError> {
    open_regular_nofollow(path)?;
    let text = std::fs::read_to_string(path).map_err(|source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let metadata =
        serde_json::from_str::<StoreMetadata>(&text).map_err(|source| StoreError::Json {
            path: path.to_path_buf(),
            source,
        })?;
    if metadata.schema_version != METADATA_SCHEMA_VERSION {
        return Err(StoreError::InvalidMetadata {
            path: path.to_path_buf(),
            reason: "unsupported metadata schema version",
        });
    }
    Ok(metadata)
}

fn hash_file(path: &Path) -> Result<ImageDigest, StoreError> {
    let mut file = open_regular_nofollow(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|source| StoreError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    ImageDigest::parse(&hex::encode(hasher.finalize())).map_err(|_| StoreError::InvalidPath {
        path: path.to_path_buf(),
        reason: "sha256 encoder produced an invalid digest",
    })
}

fn open_regular_nofollow(path: &Path) -> Result<File, StoreError> {
    reject_symlink(path)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(OFlag::O_NOFOLLOW.bits())
        .open(path)
        .map_err(|source| StoreError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let metadata = file.metadata().map_err(|source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.file_type().is_file() || metadata.file_type().is_block_device() {
        return Err(StoreError::InvalidPath {
            path: path.to_path_buf(),
            reason: "path must be a regular file",
        });
    }
    Ok(file)
}

fn reject_symlink(path: &Path) -> Result<(), StoreError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() {
        return Err(StoreError::InvalidPath {
            path: path.to_path_buf(),
            reason: "path must not be a symlink",
        });
    }
    Ok(())
}
