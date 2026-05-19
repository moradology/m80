//! Template identity and validated input types.

use std::cmp::Ordering as CmpOrdering;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest as _, Sha256};

use crate::{GuestMountPath, HookSpecSet, JailBackingPath, TemplateStoreError};

/// Opaque reference to a verified snapshot-template body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateRef {
    fingerprint: TemplateFingerprint,
}

impl TemplateRef {
    pub(crate) fn new(fingerprint: TemplateFingerprint) -> Self {
        Self { fingerprint }
    }

    /// Return this template's fingerprint.
    #[must_use]
    pub fn fingerprint(&self) -> &TemplateFingerprint {
        &self.fingerprint
    }
}

/// Versioned snapshot-template fingerprint.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TemplateFingerprint([u8; 32]);

impl TemplateFingerprint {
    /// Parse a lowercase hex fingerprint.
    pub fn parse_hex(value: &str) -> Result<Self, TemplateStoreError> {
        if value.len() != 64 {
            return Err(TemplateStoreError::InvalidValue {
                field: "template.fingerprint",
                reason: "fingerprint must be 64 lowercase hex characters",
            });
        }
        if !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(TemplateStoreError::InvalidValue {
                field: "template.fingerprint",
                reason: "fingerprint must be lowercase hex",
            });
        }
        let mut out = [0_u8; 32];
        hex::decode_to_slice(value, &mut out).map_err(|_| TemplateStoreError::InvalidValue {
            field: "template.fingerprint",
            reason: "fingerprint must be valid hex",
        })?;
        Ok(Self(out))
    }

    /// Compute the Phase D v1 template fingerprint.
    #[must_use]
    pub fn compute(inputs: &TemplateInputs) -> Self {
        let mut hasher = Sha256::new();
        hash_field(&mut hasher, "version", b"TemplateFingerprintV1");
        hash_field(
            &mut hasher,
            "host_kernel_version",
            inputs.host_kernel_version.as_bytes(),
        );
        hash_field(
            &mut hasher,
            "firecracker_version",
            inputs.firecracker_version.as_bytes(),
        );
        hash_field(
            &mut hasher,
            "guest_kernel_digest",
            inputs.guest_kernel_digest.as_str().as_bytes(),
        );

        let mut pmem_layers = inputs.pmem_image_digest_set.clone();
        pmem_layers.sort_by(PmemTemplateEntry::canonical_cmp);
        hash_field(
            &mut hasher,
            "pmem_len",
            &(pmem_layers.len() as u64).to_be_bytes(),
        );
        for layer in &pmem_layers {
            hash_field(
                &mut hasher,
                "pmem_mount_at",
                layer.mount_at.as_path().to_string_lossy().as_bytes(),
            );
            hash_field(
                &mut hasher,
                "pmem_digest",
                layer.image_digest.as_str().as_bytes(),
            );
            hash_field(
                &mut hasher,
                "pmem_sharing",
                layer.sharing.as_str().as_bytes(),
            );
            hash_field(
                &mut hasher,
                "pmem_jail_path",
                layer
                    .jail_backing_path
                    .as_path()
                    .to_string_lossy()
                    .as_bytes(),
            );
        }

        hash_field(
            &mut hasher,
            "post_init_state_digest",
            inputs.post_init_state_digest.as_str().as_bytes(),
        );
        hash_field(
            &mut hasher,
            "hook_spec_set_digest",
            inputs.hook_spec_set.digest().as_str().as_bytes(),
        );
        Self(hasher.finalize().into())
    }

    /// Return the raw 32-byte fingerprint.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Return the lowercase hex representation.
    #[must_use]
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl fmt::Debug for TemplateFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TemplateFingerprint")
            .field(&self.to_hex())
            .finish()
    }
}

impl fmt::Display for TemplateFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl Serialize for TemplateFingerprint {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for TemplateFingerprint {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse_hex(&value).map_err(serde::de::Error::custom)
    }
}

/// Typed inputs that define a snapshot template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateInputs {
    host_kernel_version: String,
    firecracker_version: String,
    guest_kernel_digest: TemplateDigest,
    pmem_image_digest_set: Vec<PmemTemplateEntry>,
    post_init_state_digest: TemplateDigest,
    hook_spec_set: HookSpecSet,
}

impl TemplateInputs {
    /// Construct typed template inputs.
    pub fn new(
        host_kernel_version: impl Into<String>,
        firecracker_version: impl Into<String>,
        guest_kernel_digest: TemplateDigest,
        pmem_image_digest_set: Vec<PmemTemplateEntry>,
        post_init_state_digest: TemplateDigest,
        hook_spec_set: HookSpecSet,
    ) -> Result<Self, TemplateStoreError> {
        let host_kernel_version =
            checked_identity_value("template.host_kernel_version", host_kernel_version.into())?;
        let firecracker_version =
            checked_identity_value("template.firecracker_version", firecracker_version.into())?;
        Ok(Self {
            host_kernel_version,
            firecracker_version,
            guest_kernel_digest,
            pmem_image_digest_set,
            post_init_state_digest,
            hook_spec_set,
        })
    }

    /// Host kernel version used when the template was built.
    #[must_use]
    pub fn host_kernel_version(&self) -> &str {
        &self.host_kernel_version
    }

    /// Firecracker version used when the template was captured.
    #[must_use]
    pub fn firecracker_version(&self) -> &str {
        &self.firecracker_version
    }

    /// Guest kernel digest used when the template was captured.
    #[must_use]
    pub fn guest_kernel_digest(&self) -> &TemplateDigest {
        &self.guest_kernel_digest
    }

    /// Deterministic pmem layer identity set.
    #[must_use]
    pub fn pmem_image_digest_set(&self) -> &[PmemTemplateEntry] {
        &self.pmem_image_digest_set
    }

    /// Post-init state digest.
    #[must_use]
    pub fn post_init_state_digest(&self) -> &TemplateDigest {
        &self.post_init_state_digest
    }

    /// Ordered hook set.
    #[must_use]
    pub fn hook_spec_set(&self) -> &HookSpecSet {
        &self.hook_spec_set
    }
}

/// Lowercase sha256 digest used for non-image template inputs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TemplateDigest(String);

impl TemplateDigest {
    /// Parse exactly 64 lowercase hexadecimal sha256 characters.
    pub fn parse(value: &str) -> Result<Self, TemplateStoreError> {
        parse_digest("template.digest", value).map(Self)
    }

    pub(crate) fn from_digest_hex(value: String) -> Self {
        Self(value)
    }

    /// Return the digest string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for TemplateDigest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for TemplateDigest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

/// Lowercase sha256 digest for a pmem image artifact.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageDigest(String);

impl ImageDigest {
    /// Parse exactly 64 lowercase hexadecimal sha256 characters.
    pub fn parse(value: &str) -> Result<Self, TemplateStoreError> {
        parse_digest("template.image_digest", value).map(Self)
    }

    /// Return the digest string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for ImageDigest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ImageDigest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

/// One pmem layer entry included in a template fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PmemTemplateEntry {
    mount_at: GuestMountPath,
    image_digest: ImageDigest,
    sharing: PmemTemplateSharing,
    jail_backing_path: JailBackingPath,
}

impl PmemTemplateEntry {
    /// Construct a pmem template entry from validated parts.
    #[must_use]
    pub fn new(
        mount_at: GuestMountPath,
        image_digest: ImageDigest,
        sharing: PmemTemplateSharing,
        jail_backing_path: JailBackingPath,
    ) -> Self {
        Self {
            mount_at,
            image_digest,
            sharing,
            jail_backing_path,
        }
    }

    /// Guest mount path.
    #[must_use]
    pub fn mount_at(&self) -> &GuestMountPath {
        &self.mount_at
    }

    /// Image digest.
    #[must_use]
    pub fn image_digest(&self) -> &ImageDigest {
        &self.image_digest
    }

    /// Template sharing mode.
    #[must_use]
    pub fn sharing(&self) -> PmemTemplateSharing {
        self.sharing
    }

    /// Stable jail-visible backing path shape.
    #[must_use]
    pub fn jail_backing_path(&self) -> &JailBackingPath {
        &self.jail_backing_path
    }

    fn canonical_cmp(left: &Self, right: &Self) -> CmpOrdering {
        left.mount_at
            .as_path()
            .cmp(right.mount_at.as_path())
            .then_with(|| left.image_digest.as_str().cmp(right.image_digest.as_str()))
            .then_with(|| left.sharing.as_str().cmp(right.sharing.as_str()))
            .then_with(|| {
                left.jail_backing_path
                    .as_path()
                    .cmp(right.jail_backing_path.as_path())
            })
    }
}

/// Pmem sharing mode recorded in a template fingerprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PmemTemplateSharing {
    /// Per-VM backing materialization.
    PerVm,
    /// Shared backing materialization.
    Shared,
}

impl PmemTemplateSharing {
    fn as_str(self) -> &'static str {
        match self {
            Self::PerVm => "per_vm",
            Self::Shared => "shared",
        }
    }
}

fn parse_digest(field: &'static str, value: &str) -> Result<String, TemplateStoreError> {
    if value.len() != 64 {
        return Err(TemplateStoreError::InvalidValue {
            field,
            reason: "sha256 digest must be 64 lowercase hex characters",
        });
    }
    if !value
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(TemplateStoreError::InvalidValue {
            field,
            reason: "sha256 digest must be lowercase hex",
        });
    }
    Ok(value.to_owned())
}

fn checked_identity_value(
    field: &'static str,
    value: String,
) -> Result<String, TemplateStoreError> {
    if value.is_empty() {
        return Err(TemplateStoreError::InvalidValue {
            field,
            reason: "must not be empty",
        });
    }
    if value.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(TemplateStoreError::InvalidValue {
            field,
            reason: "must not contain control characters",
        });
    }
    Ok(value)
}

fn hash_field(hasher: &mut Sha256, label: &str, value: &[u8]) {
    hasher.update(label.as_bytes());
    hasher.update([0]);
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}
