//! Post-restore hook request/response payload types.

use crate::error::ProtoError;

/// Wire `kind` value for an envelope carrying [`PostRestoreHookRequest`].
pub const PAYLOAD_KIND_POST_RESTORE_HOOK_REQUEST: &str = "post_restore_hook_request";

/// Wire `kind` value for an envelope carrying [`PostRestoreHookResponse`].
pub const PAYLOAD_KIND_POST_RESTORE_HOOK_RESPONSE: &str = "post_restore_hook_response";

/// Host-to-guest request to run post-restore hooks before lease hand-back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostRestoreHookRequest {
    /// Host-generated restore nonce mixed into guest entropy before reseed.
    pub restore_nonce: [u8; 32],
    /// Ordered closed hook list.
    pub hooks: Vec<HookKindWire>,
}

/// Guest-to-host response for post-restore hook execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostRestoreHookResponse {
    /// One result per requested hook, in execution order.
    pub results: Vec<HookResultWire>,
}

/// Closed post-restore hook kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookKindWire {
    /// Rewrite systemd random-seed state if present.
    ReseedSystemdRandomSeed,
    /// Regenerate `/etc/machine-id`.
    RegenMachineId,
    /// Set the guest hostname.
    SetHostname {
        /// Validated RFC-1123 hostname.
        hostname: HookHostname,
    },
}

impl HookKindWire {
    /// Construct a hostname hook after validating the hostname.
    pub fn set_hostname(hostname: impl Into<String>) -> Result<Self, ProtoError> {
        Ok(Self::SetHostname {
            hostname: HookHostname::new(hostname)?,
        })
    }
}

/// One post-restore hook result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookResultWire {
    /// Hook whose execution produced this result.
    pub kind: HookKindWire,
    /// Success or failure status.
    pub status: HookStatus,
    /// Typed failure detail. Must be `None` when status is `Succeeded`.
    pub error: Option<HookError>,
}

/// Post-restore hook execution status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookStatus {
    /// Hook completed successfully.
    Succeeded,
    /// Hook failed; see [`HookResultWire::error`].
    Failed,
}

/// Typed post-restore hook failure modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookError {
    /// Guest entropy reseed failed.
    ReseedFailed,
    /// Machine-id write or regeneration failed.
    MachineIdWriteFailed,
    /// `sethostname(2)` failed with `errno`.
    HostnameSyscallFailed {
        /// Hostname syscall errno.
        errno: i32,
    },
    /// Persistent hostname file write failed.
    HostnameWriteFailed,
    /// systemd random-seed write failed.
    RandomSeedWriteFailed,
    /// Hostname failed validation at the guest boundary.
    InvalidHostname,
}

/// Validated RFC-1123 hostname carried by [`HookKindWire::SetHostname`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HookHostname(String);

impl HookHostname {
    /// Validate and construct a hostname.
    pub fn new(value: impl Into<String>) -> Result<Self, ProtoError> {
        let value = value.into();
        validate_hostname(&value)?;
        Ok(Self(value))
    }

    /// Return the hostname string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn into_string(self) -> String {
        self.0
    }
}

fn validate_hostname(value: &str) -> Result<(), ProtoError> {
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
    Ok(())
}

fn invalid_hostname(reason: &'static str) -> Result<(), ProtoError> {
    Err(ProtoError::MalformedPayload(format!(
        "invalid post-restore hostname: {reason}"
    )))
}
