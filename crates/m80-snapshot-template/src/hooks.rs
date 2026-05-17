//! Post-restore hook schema.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest as _, Sha256};

use crate::{TemplateDigest, TemplateStoreError};

/// Canonical ordered post-restore hook set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookSpecSet {
    hooks: Vec<HookSpec>,
    digest: TemplateDigest,
}

impl HookSpecSet {
    /// Construct a hook set preserving caller order.
    #[must_use]
    pub fn new(hooks: Vec<HookSpec>) -> Self {
        let digest = digest_hooks(&hooks);
        Self { hooks, digest }
    }

    /// Return an empty hook set.
    #[must_use]
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    /// Ordered hooks.
    #[must_use]
    pub fn hooks(&self) -> &[HookSpec] {
        &self.hooks
    }

    /// Digest over the ordered hook set.
    #[must_use]
    pub fn digest(&self) -> &TemplateDigest {
        &self.digest
    }
}

impl Serialize for HookSpecSet {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        HookSpecSetWire {
            hooks: self.hooks.clone(),
            digest: self.digest.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for HookSpecSet {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = HookSpecSetWire::deserialize(deserializer)?;
        let computed = HookSpecSet::new(wire.hooks);
        if computed.digest != wire.digest {
            return Err(serde::de::Error::custom(
                "hook spec set digest does not match hooks",
            ));
        }
        Ok(computed)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HookSpecSetWire {
    hooks: Vec<HookSpec>,
    digest: TemplateDigest,
}

/// Closed post-restore hook variants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookSpec {
    /// Rewrite systemd random-seed state if present.
    ReseedSystemdRandomSeed,
    /// Regenerate `/etc/machine-id`.
    RegenMachineId,
    /// Set the guest hostname.
    SetHostname(HostnameSpec),
}

/// Validated RFC-1123 hostname.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HostnameSpec(String);

impl HostnameSpec {
    /// Validate and construct a hostname.
    pub fn new(value: &str) -> Result<Self, TemplateStoreError> {
        if value.is_empty() {
            return invalid_hostname("hostname must not be empty");
        }
        if value.len() > 253 {
            return invalid_hostname("hostname must be <=253 bytes");
        }
        for label in value.split('.') {
            if label.is_empty() {
                return invalid_hostname("hostname labels must not be empty");
            }
            if label.len() > 63 {
                return invalid_hostname("hostname labels must be <=63 bytes");
            }
            if label.starts_with('-') {
                return invalid_hostname("hostname labels must not start with hyphen");
            }
            if label.ends_with('-') {
                return invalid_hostname("hostname labels must not end with hyphen");
            }
            if !label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            {
                return invalid_hostname(
                    "hostname labels must contain only ASCII letters, digits, and hyphen",
                );
            }
        }
        Ok(Self(value.to_owned()))
    }

    /// Return the validated hostname.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for HostnameSpec {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for HostnameSpec {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::new(&value).map_err(serde::de::Error::custom)
    }
}

fn invalid_hostname(reason: &'static str) -> Result<HostnameSpec, TemplateStoreError> {
    Err(TemplateStoreError::InvalidValue {
        field: "hook.hostname",
        reason,
    })
}

fn digest_hooks(hooks: &[HookSpec]) -> TemplateDigest {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, "version", b"HookSpecSetV1");
    hash_usize(&mut hasher, "hook_len", hooks.len());
    for hook in hooks {
        match hook {
            HookSpec::ReseedSystemdRandomSeed => {
                hash_field(&mut hasher, "hook", b"reseed_systemd_random_seed");
            }
            HookSpec::RegenMachineId => {
                hash_field(&mut hasher, "hook", b"regen_machine_id");
            }
            HookSpec::SetHostname(hostname) => {
                hash_field(&mut hasher, "hook", b"set_hostname");
                hash_field(&mut hasher, "hostname", hostname.as_str().as_bytes());
            }
        }
    }
    TemplateDigest::from_digest_hex(hex::encode(hasher.finalize()))
}

fn hash_field(hasher: &mut Sha256, label: &str, value: &[u8]) {
    hasher.update(label.as_bytes());
    hasher.update([0]);
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn hash_usize(hasher: &mut Sha256, label: &str, value: usize) {
    hash_field(hasher, label, &(value as u64).to_be_bytes());
}
