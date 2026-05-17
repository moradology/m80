//! Typed pmem layer admission surface.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::error::{ConfigError, FcError};

/// Maximum pmem layers admitted by the Phase B API surface.
pub const MAX_PMEM_LAYERS: usize = 8;

/// Content digest for an admitted image artifact.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageDigest(String);

impl ImageDigest {
    /// Parse a lowercase sha256 digest.
    pub fn parse(value: &str) -> Result<Self, FcError> {
        if value.len() != 64 {
            return Err(FcError::Config(ConfigError::DigestInvalid {
                reason: "sha256 digest must be 64 lowercase hex characters",
            }));
        }
        if !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(FcError::Config(ConfigError::DigestInvalid {
                reason: "sha256 digest must be lowercase hex",
            }));
        }
        Ok(Self(value.to_owned()))
    }

    /// Return the normalized digest string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Validated in-guest mount destination for a pmem layer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GuestMountPath(PathBuf);

impl GuestMountPath {
    /// Parse a guest mount path under `/opt/m80-layers/<name>`.
    pub fn parse(value: &str) -> Result<Self, FcError> {
        if value.is_empty() {
            return Err(FcError::Config(ConfigError::MountPathInvalid {
                reason: "path must not be empty",
            }));
        }

        let path = Path::new(value);
        if !path.is_absolute() {
            return Err(FcError::Config(ConfigError::MountPathInvalid {
                reason: "path must be absolute",
            }));
        }
        if shadows_reserved_mount(path) {
            return Err(FcError::Config(ConfigError::MountPathShadowsReserved {
                path: path.to_path_buf(),
            }));
        }

        let raw_components = value.split('/').collect::<Vec<_>>();
        if raw_components.len() != 4
            || raw_components[0] != ""
            || raw_components[1] != "opt"
            || raw_components[2] != "m80-layers"
        {
            return Err(FcError::Config(ConfigError::MountPathInvalid {
                reason: "path must match /opt/m80-layers/<name>",
            }));
        }

        let name = raw_components[3];
        if name == "." || name == ".." {
            return Err(FcError::Config(ConfigError::MountPathInvalid {
                reason: "path must not contain . or .. components",
            }));
        }

        if name.is_empty() || name.len() > 64 {
            return Err(FcError::Config(ConfigError::MountPathInvalid {
                reason: "layer name must be 1..=64 characters",
            }));
        }
        if !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        {
            return Err(FcError::Config(ConfigError::MountPathInvalid {
                reason: "layer name must contain only [A-Za-z0-9._-]",
            }));
        }

        Ok(Self(path.to_path_buf()))
    }

    /// Return the validated guest path.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

fn shadows_reserved_mount(path: &Path) -> bool {
    if path == Path::new("/") {
        return true;
    }

    [
        "/proc",
        "/sys",
        "/dev",
        "/etc",
        "/lower",
        "/upper",
        "/merged",
        "/workspace",
        "/snapshot",
    ]
    .iter()
    .map(Path::new)
    .any(|reserved| path == reserved || path.starts_with(reserved))
}

/// Store reference to a verified erofs image artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErofsImageRef {
    digest: ImageDigest,
}

impl ErofsImageRef {
    /// Construct an erofs image reference from an already-validated digest.
    #[must_use]
    pub fn from_digest(digest: ImageDigest) -> Self {
        Self { digest }
    }

    /// Return the referenced image digest.
    #[must_use]
    pub fn digest(&self) -> &ImageDigest {
        &self.digest
    }
}

/// Explicit acknowledgement that all guests sharing one pmem backing are in
/// the same trust domain.
///
/// Construction is intentional and finite: there is no [`Default`] impl and no
/// free-form string reason.
///
/// ```compile_fail
/// use m80_firecracker::TrustDomainAck;
///
/// let _ = TrustDomainAck::default();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrustDomainAck {
    reason: TrustReason,
}

impl TrustDomainAck {
    /// Construct a shared-pmem trust-domain acknowledgement.
    #[must_use]
    pub fn new(reason: TrustReason) -> Self {
        Self { reason }
    }

    /// Return the finite reason supplied by the caller.
    #[must_use]
    pub fn reason(&self) -> TrustReason {
        self.reason
    }
}

/// Finite caller reasons accepted for shared pmem backing reuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustReason {
    /// All guests are operated by the same administrative authority.
    SameOperator,
    /// All guests belong to one Kubernetes namespace boundary.
    KubernetesSameNamespace,
    /// Experimental workloads are intentionally grouped in one research
    /// sandbox trust boundary.
    ResearchSandbox,
}

impl TrustReason {
    /// Stable list of accepted trust-domain reasons.
    #[must_use]
    pub fn variants() -> &'static [Self] {
        &[
            Self::SameOperator,
            Self::KubernetesSameNamespace,
            Self::ResearchSandbox,
        ]
    }
}

/// Pmem backing lifecycle policy.
///
/// Shared backing reuse requires an explicit [`TrustDomainAck`].
///
/// ```compile_fail
/// use m80_firecracker::PmemSharing;
///
/// let sharing: PmemSharing = PmemSharing::Shared;
/// ```
///
/// Shared backing reuse accepts no caller-provided writability hint.
///
/// ```compile_fail
/// use m80_firecracker::{PmemSharing, TrustDomainAck, TrustReason};
///
/// let _ = PmemSharing::Shared(
///     TrustDomainAck::new(TrustReason::SameOperator),
///     true,
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PmemSharing {
    /// Backing is materialized per VM.
    PerVm,
    /// Backing is reused across VMs in one acknowledged trust domain.
    Shared(TrustDomainAck),
}

/// One read-only erofs layer attached through Firecracker virtio-pmem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PmemLayer {
    image: ErofsImageRef,
    sharing: PmemSharing,
    mount_at: GuestMountPath,
}

impl PmemLayer {
    /// Construct a pmem layer from validated parts.
    #[must_use]
    pub fn new(image: ErofsImageRef, sharing: PmemSharing, mount_at: GuestMountPath) -> Self {
        Self {
            image,
            sharing,
            mount_at,
        }
    }

    /// Return the erofs image reference.
    #[must_use]
    pub fn image(&self) -> &ErofsImageRef {
        &self.image
    }

    /// Return the sharing policy.
    #[must_use]
    pub fn sharing(&self) -> PmemSharing {
        self.sharing
    }

    /// Return the guest mount destination.
    #[must_use]
    pub fn mount_at(&self) -> &GuestMountPath {
        &self.mount_at
    }
}

/// Validate the admitted pmem layer collection.
pub fn validate_pmem_layers(layers: &[PmemLayer]) -> Result<(), FcError> {
    if layers.len() > MAX_PMEM_LAYERS {
        return Err(FcError::Config(ConfigError::TooManyLayers {
            max: MAX_PMEM_LAYERS,
            got: layers.len(),
        }));
    }

    let mut seen = HashSet::with_capacity(layers.len());
    for layer in layers {
        if !seen.insert(layer.mount_at().as_path().to_path_buf()) {
            return Err(FcError::Config(ConfigError::MountPathDuplicated {
                path: layer.mount_at().as_path().to_path_buf(),
            }));
        }
    }
    Ok(())
}
