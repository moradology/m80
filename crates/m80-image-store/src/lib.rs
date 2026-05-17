//! Content-addressed store for pre-built m80 image artifacts.
//!
//! This crate ingests and verifies host-side filesystem images. It does not
//! decide how production images are built.

#![deny(missing_docs)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

mod build;
mod erofs;
mod lock;
mod store;

pub use store::{ImageRecord, ImageStore, ImageTemplateCoordinationGuard, SharedImageRef};

/// Default host image-store root.
pub const DEFAULT_STORE_ROOT: &str = "/var/lib/m80-images";

/// Lowercase sha256 digest used as the image-store content address.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageDigest(String);

impl ImageDigest {
    /// Parse exactly 64 lowercase hexadecimal sha256 characters.
    pub fn parse(value: &str) -> Result<Self, ImageDigestParseError> {
        if value.len() != 64 {
            return Err(ImageDigestParseError {
                reason: "sha256 digest must be 64 lowercase hex characters",
            });
        }
        if !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(ImageDigestParseError {
                reason: "sha256 digest must be lowercase hex",
            });
        }
        Ok(Self(value.to_owned()))
    }

    /// Return the digest string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Digest parse failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid image digest: {reason}")]
pub struct ImageDigestParseError {
    /// Finite rejection reason.
    pub reason: &'static str,
}

/// A resolved image artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageArtifact {
    /// Erofs filesystem image.
    Erofs(ErofsImage),
    /// Ext4 filesystem image.
    Ext4(Ext4Image),
}

impl ImageArtifact {
    /// Return the artifact kind.
    #[must_use]
    pub fn kind(&self) -> ImageKind {
        match self {
            Self::Erofs(_) => ImageKind::Erofs,
            Self::Ext4(_) => ImageKind::Ext4,
        }
    }

    /// Return the canonical artifact path.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::Erofs(image) => image.path(),
            Self::Ext4(image) => image.path(),
        }
    }

    /// Return the artifact size in bytes.
    #[must_use]
    pub fn size_bytes(&self) -> u64 {
        match self {
            Self::Erofs(image) => image.size_bytes(),
            Self::Ext4(image) => image.size_bytes(),
        }
    }

    /// Return the artifact digest.
    #[must_use]
    pub fn digest(&self) -> &ImageDigest {
        match self {
            Self::Erofs(image) => image.digest(),
            Self::Ext4(image) => image.digest(),
        }
    }
}

/// Filesystem artifact kind stored by [`ImageStore`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum ImageKind {
    /// Erofs read-only filesystem image.
    Erofs,
    /// Ext4 filesystem image.
    Ext4,
}

impl ImageKind {
    /// Return the canonical artifact filename for this kind.
    #[must_use]
    pub fn file_name(self) -> &'static str {
        match self {
            Self::Erofs => "image.erofs",
            Self::Ext4 => "image.ext4",
        }
    }

    /// Return the kind name used in diagnostics.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Erofs => "erofs",
            Self::Ext4 => "ext4",
        }
    }
}

impl std::fmt::Display for ImageKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Resolved erofs artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErofsImage {
    path: PathBuf,
    size_bytes: u64,
    digest: ImageDigest,
}

impl ErofsImage {
    pub(crate) fn new(path: PathBuf, size_bytes: u64, digest: ImageDigest) -> Self {
        Self {
            path,
            size_bytes,
            digest,
        }
    }

    /// Canonical host path to the image file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Artifact size in bytes.
    #[must_use]
    pub fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    /// Artifact sha256 digest.
    #[must_use]
    pub fn digest(&self) -> &ImageDigest {
        &self.digest
    }
}

/// Resolved ext4 artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ext4Image {
    path: PathBuf,
    size_bytes: u64,
    digest: ImageDigest,
}

impl Ext4Image {
    pub(crate) fn new(path: PathBuf, size_bytes: u64, digest: ImageDigest) -> Self {
        Self {
            path,
            size_bytes,
            digest,
        }
    }

    /// Canonical host path to the image file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Artifact size in bytes.
    #[must_use]
    pub fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    /// Artifact sha256 digest.
    #[must_use]
    pub fn digest(&self) -> &ImageDigest {
        &self.digest
    }
}

/// Store operation failure.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Store root or caller path failed validation.
    #[error("invalid path {}: {reason}", path.display())]
    InvalidPath {
        /// Rejected path.
        path: PathBuf,
        /// Finite rejection reason.
        reason: &'static str,
    },
    /// Stored bytes did not match the requested digest.
    #[error("digest mismatch: expected {expected:?}, got {got:?}")]
    DigestMismatch {
        /// Expected digest.
        expected: ImageDigest,
        /// Observed digest.
        got: ImageDigest,
    },
    /// No artifact exists for the digest.
    #[error("image artifact not found for digest {digest:?}")]
    NotFound {
        /// Missing digest.
        digest: ImageDigest,
    },
    /// More than one kind exists for a digest and the caller did not choose.
    #[error("image digest {digest:?} resolves to multiple artifact kinds")]
    AmbiguousDigest {
        /// Ambiguous digest.
        digest: ImageDigest,
    },
    /// A shared-image active-use marker already exists for this VM.
    #[error("shared image ref already exists: digest={digest:?} vm_id={vm_id}")]
    SharedRefAlreadyExists {
        /// Referenced digest.
        digest: ImageDigest,
        /// VM id whose marker already exists.
        vm_id: String,
    },
    /// A shared-image active-use marker was missing when release was requested.
    #[error("shared image ref not found: digest={digest:?} vm_id={vm_id}")]
    SharedRefNotFound {
        /// Referenced digest.
        digest: ImageDigest,
        /// VM id whose marker was missing.
        vm_id: String,
    },
    /// An image cannot be removed while VMs still hold active-use markers.
    #[error("image digest {digest:?} is actively used by {ref_count} shared refs")]
    ImageInUse {
        /// Digest requested for removal.
        digest: ImageDigest,
        /// Number of active shared-use markers.
        ref_count: usize,
    },
    /// An image cannot be removed while snapshot templates reference it.
    #[error(
        "image digest {digest:?} is referenced by snapshot templates: {template_fingerprints:?}"
    )]
    ImageReferencedByTemplate {
        /// Digest requested for removal.
        digest: ImageDigest,
        /// Referencing template fingerprints.
        template_fingerprints: Vec<String>,
    },
    /// Host I/O failed at a concrete path.
    #[error("image-store io at {}: {source}", path.display())]
    Io {
        /// Path being accessed.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// Metadata JSON could not be read or written.
    #[error("image-store metadata json at {}: {source}", path.display())]
    Json {
        /// Metadata path.
        path: PathBuf,
        /// Underlying JSON error.
        #[source]
        source: serde_json::Error,
    },
    /// Metadata shape was not internally consistent.
    #[error("invalid image-store metadata at {}: {reason}", path.display())]
    InvalidMetadata {
        /// Metadata path.
        path: PathBuf,
        /// Finite rejection reason.
        reason: &'static str,
    },
    /// A filesystem creation helper failed.
    #[error("mkfs for {kind} failed: {detail}")]
    MkfsFailed {
        /// Image kind being built.
        kind: ImageKind,
        /// Captured stderr or spawn failure detail.
        detail: String,
    },
    /// Host erofs probing failed before an artifact was admitted.
    #[error("erofs probe failed for {}: {detail}", path.display())]
    ErofsProbeFailed {
        /// Erofs artifact being probed.
        path: PathBuf,
        /// Captured stderr, spawn failure, or parse failure detail.
        detail: String,
    },
    /// The erofs image uses a feature outside the pinned guest kernel floor.
    #[error("erofs image {} uses unsupported feature {feature}", path.display())]
    UnsupportedErofsFeature {
        /// Erofs artifact being probed.
        path: PathBuf,
        /// Rejected feature token from `dump.erofs -s`.
        feature: String,
    },
    /// The erofs image uses a compressor outside the pinned guest kernel floor.
    #[error(
        "erofs image {} uses unsupported compression algorithm {algorithm}",
        path.display()
    )]
    UnsupportedErofsCompression {
        /// Erofs artifact being probed.
        path: PathBuf,
        /// Rejected compression algorithm from `dump.erofs -s`.
        algorithm: String,
    },
}
