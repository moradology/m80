//! Error → exit-code mapping and JSON error envelope for `m80`.
//!
//! Each `FcError` variant maps to a stable, distinct exit code so callers
//! can branch on error class without parsing stderr text.
//!
//! Behavior captures: bead `m80-4ef.3.1` (exit-code map),
//! `m80-4ef.3.2` (JSON envelope), `m80-4ef.3.3` (stderr informational).

use serde::{Deserialize, Serialize};

use m80_firecracker::FcError;

use crate::json;
use crate::request_id;

// =====================================================================
// Exit-code constants (stable per variant — do not renumber)
// =====================================================================

/// Generic / unclassified error.
pub(crate) const EXIT_GENERIC: i32 = 1;
/// Preflight check failed.
pub(crate) const EXIT_PREFLIGHT: i32 = 2;
/// Admission refused (concurrency limit).
pub(crate) const EXIT_ADMISSION: i32 = 3;
/// Manifest read/validate failed.
pub(crate) const EXIT_MANIFEST: i32 = 4;
/// Invalid lifecycle state.
pub(crate) const EXIT_INVALID_STATE: i32 = 5;
/// Configuration loading or merging failure.
pub(crate) const EXIT_CONFIG: i32 = 6;
/// Feature explicitly not implemented in v0.1 (e.g., `m80 exec` CLI stub
/// pending the v0.2 out-of-process IPC). Distinct from `EXIT_INVALID_STATE`
/// so callers can branch on "feature gap" vs "lifecycle bug".
pub(crate) const EXIT_NOT_IMPLEMENTED: i32 = 7;
/// Warm pool had no ready slot and does not cold-boot as fallback.
pub(crate) const EXIT_POOL_EMPTY: i32 = 8;
/// API socket or guestd ready handshake timed out.
pub(crate) const EXIT_TIMEOUT: i32 = 9;
/// Run-dir ownership conflict or not found.
pub(crate) const EXIT_RUN_DIR_OWNERSHIP: i32 = 10;
/// VM idle watchdog fired; session timed out.
pub(crate) const EXIT_IDLE_TIMED_OUT: i32 = 11;
/// One-shot VM was already consumed; cannot reuse.
pub(crate) const EXIT_ONE_SHOT_CONSUMED: i32 = 12;

/// Map an [`FcError`] to its stable CLI exit code.
///
/// The mapping is intentionally one-to-one at the variant level so
/// downstream scripts can `case $?` without parsing stderr.
pub(crate) fn exit_code_for(err: &FcError) -> i32 {
    match err {
        FcError::Preflight(_) => EXIT_PREFLIGHT,
        FcError::AdmissionRefused { .. } => EXIT_ADMISSION,
        FcError::PoolEmpty { .. } => EXIT_POOL_EMPTY,
        FcError::Manifest(_) => EXIT_MANIFEST,
        FcError::InvalidState { .. } => EXIT_INVALID_STATE,
        FcError::Config(_) => EXIT_CONFIG,
        FcError::UnsupportedOperation { .. } => EXIT_NOT_IMPLEMENTED,
        FcError::ApiSocketTimeout { .. } | FcError::GuestdReadyTimeout { .. } => EXIT_TIMEOUT,
        FcError::RunDirOwnershipAmbiguous { .. }
        | FcError::RunDirAlreadyOwned { .. }
        | FcError::RunDirNotFound { .. } => EXIT_RUN_DIR_OWNERSHIP,
        FcError::IdleTimedOut => EXIT_IDLE_TIMED_OUT,
        FcError::OneShotConsumed => EXIT_ONE_SHOT_CONSUMED,
        // Storage, Jailer, Network, Client, Vsock, Io, and typed runtime
        // cleanup/serialization failures are all "something went wrong at
        // runtime" — generic.
        FcError::Storage(_)
        | FcError::Jailer(_)
        | FcError::Cgroup(_)
        | FcError::Network(_)
        | FcError::Client(_)
        | FcError::Vsock(_)
        | FcError::Protocol(_)
        | FcError::Snapshot(_)
        | FcError::FileOp(_)
        | FcError::DriveHotplug(_)
        | FcError::TenantIdentityMismatch { .. }
        | FcError::Io(_)
        | FcError::PathIo { .. }
        | FcError::Json { .. }
        | FcError::CommandSpawnFailed { .. }
        | FcError::CommandFailed { .. }
        | FcError::ArtifactMissing { .. }
        | FcError::WarmPoolFillFailed { .. }
        | FcError::WarmReadyProbeRejected { .. }
        | FcError::WarmReadyProbeNoResult
        | FcError::WarmOwnerSocketExists { .. }
        | FcError::WarmOwnerNotAcceptingLeases
        | FcError::WarmOwnerDrainTimeout { .. }
        | FcError::WarmCompatibilityMismatch { .. }
        | FcError::UnexpectedWarmResponse { .. }
        | FcError::KillFailed { .. }
        | FcError::ReapTimeout { .. }
        | FcError::ReapFailed { .. } => EXIT_GENERIC,
    }
}

// =====================================================================
// JSON error payload (m80-4ef.3.2)
// =====================================================================

/// Machine-readable error payload emitted on stderr inside the shared
/// versioned JSON envelope when `--json` is set.
///
/// The `variant` field is the stable Rust variant name callers may match.
/// The `detail` field is the `Display` rendering of the full error chain.
/// The `exit_code` field mirrors the process exit code so callers that
/// capture stderr have both pieces in one object.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ErrorEnvelope {
    /// Error class / variant name (e.g., `"Preflight"`, `"Config"`).
    pub(crate) variant: &'static str,
    /// Full human-readable description.
    pub(crate) detail: String,
    /// Corresponding CLI exit code.
    pub(crate) exit_code: i32,
    /// Number of warm slots that will be ready, when the error is `PoolEmpty`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) target_ready: Option<usize>,
}

/// Build an [`ErrorEnvelope`] from an [`FcError`].
pub(crate) fn envelope(err: &FcError) -> ErrorEnvelope {
    let target_ready = match err {
        FcError::PoolEmpty { target_ready } => Some(*target_ready),
        _ => None,
    };
    ErrorEnvelope {
        variant: variant_name(err),
        detail: err.to_string(),
        exit_code: exit_code_for(err),
        target_ready,
    }
}


/// Return the stable variant name string for an [`FcError`].
fn variant_name(err: &FcError) -> &'static str {
    match err {
        FcError::Preflight(_) => "Preflight",
        FcError::Manifest(_) => "Manifest",
        FcError::Storage(_) => "Storage",
        FcError::Jailer(_) => "Jailer",
        FcError::Cgroup(_) => "Cgroup",
        FcError::Network(_) => "Network",
        FcError::Client(_) => "Client",
        FcError::Vsock(_) => "Vsock",
        FcError::Protocol(_) => "Protocol",
        FcError::AdmissionRefused { .. } => "AdmissionRefused",
        FcError::PoolEmpty { .. } => "PoolEmpty",
        FcError::InvalidState { .. } => "InvalidState",
        FcError::ApiSocketTimeout { .. } => "ApiSocketTimeout",
        FcError::GuestdReadyTimeout { .. } => "GuestdReadyTimeout",
        FcError::RunDirOwnershipAmbiguous { .. } => "RunDirOwnershipAmbiguous",
        FcError::RunDirAlreadyOwned { .. } => "RunDirAlreadyOwned",
        FcError::RunDirNotFound { .. } => "RunDirNotFound",
        FcError::PathIo { .. } => "PathIo",
        FcError::Json { .. } => "Json",
        FcError::UnsupportedOperation { .. } => "UnsupportedOperation",
        FcError::CommandSpawnFailed { .. } => "CommandSpawnFailed",
        FcError::CommandFailed { .. } => "CommandFailed",
        FcError::ArtifactMissing { .. } => "ArtifactMissing",
        FcError::WarmPoolFillFailed { .. } => "WarmPoolFillFailed",
        FcError::WarmReadyProbeRejected { .. } => "WarmReadyProbeRejected",
        FcError::WarmReadyProbeNoResult => "WarmReadyProbeNoResult",
        FcError::WarmOwnerSocketExists { .. } => "WarmOwnerSocketExists",
        FcError::WarmOwnerNotAcceptingLeases => "WarmOwnerNotAcceptingLeases",
        FcError::WarmOwnerDrainTimeout { .. } => "WarmOwnerDrainTimeout",
        FcError::WarmCompatibilityMismatch { .. } => "WarmCompatibilityMismatch",
        FcError::UnexpectedWarmResponse { .. } => "UnexpectedWarmResponse",
        FcError::KillFailed { .. } => "KillFailed",
        FcError::ReapTimeout { .. } => "ReapTimeout",
        FcError::ReapFailed { .. } => "ReapFailed",
        FcError::Io(_) => "Io",
        FcError::Config(_) => "Config",
        FcError::Snapshot(_) => "Snapshot",
        FcError::FileOp(_) => "FileOp",
        FcError::DriveHotplug(_) => "DriveHotplug",
        FcError::TenantIdentityMismatch { .. } => "TenantIdentityMismatch",
        FcError::IdleTimedOut => "IdleTimedOut",
        FcError::OneShotConsumed => "OneShotConsumed",
    }
}

/// Render the error to stderr (shared JSON envelope when `json` is true;
/// plain `Display` otherwise) and return the exit code.
///
/// Non-error progress / log lines may already be on stderr (that is fine;
/// stderr is informational — bead m80-4ef.3.3). Only the error itself is
/// written here, always last.
pub(crate) fn render_error(err: &FcError, json: bool) -> i32 {
    if json {
        let env = envelope(err);
        // Unwrap: serializing a struct of strings cannot fail.
        eprintln!("{}", json::to_pretty(&env));
    } else if let Some(request_id) = request_id::current() {
        eprintln!("error: [{request_id}] {err}");
        if let FcError::Preflight(pe) = err {
            eprintln!("hint: {}", pe.hint());
        }
    } else {
        eprintln!("error: {err}");
        if let FcError::Preflight(pe) = err {
            eprintln!("hint: {}", pe.hint());
        }
    }
    exit_code_for(err)
}

#[cfg(test)]
mod tests {
    use m80_firecracker::ConfigError;

    use super::*;

    // Each variant must produce a distinct exit code that is non-zero.
    #[test]
    fn preflight_is_2() {
        use m80_preflight::PreflightError;
        let err = FcError::Preflight(PreflightError::KvmUnavailable {
            path: "/dev/kvm".into(),
        });
        assert_eq!(exit_code_for(&err), EXIT_PREFLIGHT);
    }

    #[test]
    fn admission_refused_is_3() {
        let err = FcError::AdmissionRefused { limit: 4 };
        assert_eq!(exit_code_for(&err), EXIT_ADMISSION);
    }

    #[test]
    fn pool_empty_is_8() {
        let err = FcError::PoolEmpty { target_ready: 1 };
        assert_eq!(exit_code_for(&err), EXIT_POOL_EMPTY);
        assert_eq!(variant_name(&err), "PoolEmpty");
    }

    #[test]
    fn config_is_6() {
        let err = FcError::Config(ConfigError::InvalidValue {
            field: "config",
            reason: "bad toml".into(),
        });
        assert_eq!(exit_code_for(&err), EXIT_CONFIG);
    }

    #[test]
    fn invalid_state_is_5() {
        let err = FcError::InvalidState {
            expected: "Running",
            actual: "Stopped",
        };
        assert_eq!(exit_code_for(&err), EXIT_INVALID_STATE);
    }

    #[test]
    fn io_is_generic() {
        use std::io;
        let err = FcError::Io(io::Error::new(io::ErrorKind::NotFound, "gone"));
        assert_eq!(exit_code_for(&err), EXIT_GENERIC);
    }

    #[test]
    fn preflight_exit_code_is_nonzero() {
        use m80_preflight::PreflightError;
        let err = FcError::Preflight(PreflightError::KvmUnavailable {
            path: "/dev/kvm".into(),
        });
        assert_ne!(exit_code_for(&err), 0);
    }

    #[test]
    fn admission_refused_exit_code_is_nonzero() {
        let err = FcError::AdmissionRefused { limit: 1 };
        assert_ne!(exit_code_for(&err), 0);
    }

    #[test]
    fn pool_empty_exit_code_is_nonzero() {
        let err = FcError::PoolEmpty { target_ready: 1 };
        assert_ne!(exit_code_for(&err), 0);
    }

    #[test]
    fn config_exit_code_is_nonzero() {
        let err = FcError::Config(ConfigError::InvalidValue {
            field: "config",
            reason: "x".into(),
        });
        assert_ne!(exit_code_for(&err), 0);
    }

    #[test]
    fn api_socket_timeout_exit_code_is_nonzero() {
        let err = FcError::ApiSocketTimeout {
            path: "/run/m80/firecracker.sock".into(),
            timeout: std::time::Duration::from_secs(5),
        };
        assert_ne!(exit_code_for(&err), 0);
    }

    #[test]
    fn guestd_ready_timeout_exit_code_is_nonzero() {
        let err = FcError::GuestdReadyTimeout {
            path: "/run/m80/vsock.sock_9000".into(),
            timeout: std::time::Duration::from_secs(60),
        };
        assert_ne!(exit_code_for(&err), 0);
    }

    #[test]
    fn invalid_state_exit_code_is_nonzero() {
        let err = FcError::InvalidState {
            expected: "a",
            actual: "b",
        };
        assert_ne!(exit_code_for(&err), 0);
    }

    #[test]
    fn io_exit_code_is_nonzero() {
        use std::io;
        let err = FcError::Io(io::Error::new(io::ErrorKind::Other, "test"));
        assert_ne!(exit_code_for(&err), 0);
    }

    #[test]
    fn file_op_exit_code_is_nonzero() {
        let err = FcError::FileOp(m80_proto::FileError::NotFound);
        assert_ne!(exit_code_for(&err), 0);
    }

    #[test]
    fn all_variants_have_distinct_codes() {
        // Codes that exist in the exit-code constants table.
        let defined = [
            EXIT_GENERIC,
            EXIT_PREFLIGHT,
            EXIT_ADMISSION,
            EXIT_MANIFEST,
            EXIT_INVALID_STATE,
            EXIT_CONFIG,
            EXIT_NOT_IMPLEMENTED,
            EXIT_POOL_EMPTY,
            EXIT_TIMEOUT,
            EXIT_RUN_DIR_OWNERSHIP,
            EXIT_IDLE_TIMED_OUT,
            EXIT_ONE_SHOT_CONSUMED,
        ];
        let mut seen = std::collections::HashSet::new();
        for code in &defined {
            assert!(seen.insert(code), "duplicate exit code: {code}");
        }
    }

    #[test]
    fn api_socket_timeout_is_9() {
        let err = FcError::ApiSocketTimeout {
            path: "/run/m80/firecracker.sock".into(),
            timeout: std::time::Duration::from_secs(5),
        };
        assert_eq!(exit_code_for(&err), EXIT_TIMEOUT);
    }

    #[test]
    fn guestd_ready_timeout_is_9() {
        let err = FcError::GuestdReadyTimeout {
            path: "/run/m80/vsock.sock_9000".into(),
            timeout: std::time::Duration::from_secs(60),
        };
        assert_eq!(exit_code_for(&err), EXIT_TIMEOUT);
    }

    #[test]
    fn run_dir_already_owned_is_10() {
        let err = FcError::RunDirAlreadyOwned { run_dir: "/run/m80/x".into(), pid: 1234 };
        assert_eq!(exit_code_for(&err), EXIT_RUN_DIR_OWNERSHIP);
    }

    #[test]
    fn run_dir_ownership_ambiguous_is_10() {
        let err = FcError::RunDirOwnershipAmbiguous { run_dir: "/run/m80/x".into() };
        assert_eq!(exit_code_for(&err), EXIT_RUN_DIR_OWNERSHIP);
    }

    #[test]
    fn run_dir_not_found_is_10() {
        let err = FcError::RunDirNotFound { vm_id: "vm0".into(), run_dir: "/run/m80/x".into() };
        assert_eq!(exit_code_for(&err), EXIT_RUN_DIR_OWNERSHIP);
    }

    #[test]
    fn idle_timed_out_is_11() {
        assert_eq!(exit_code_for(&FcError::IdleTimedOut), EXIT_IDLE_TIMED_OUT);
    }

    #[test]
    fn one_shot_consumed_is_12() {
        assert_eq!(exit_code_for(&FcError::OneShotConsumed), EXIT_ONE_SHOT_CONSUMED);
    }

    #[test]
    fn json_envelope_fields() {
        let err = FcError::Config(ConfigError::InvalidValue {
            field: "config",
            reason: "bad".into(),
        });
        let env = envelope(&err);
        assert_eq!(env.variant, "Config");
        assert_eq!(env.exit_code, EXIT_CONFIG);
        assert!(!env.detail.is_empty());
    }

    #[test]
    fn json_envelope_is_stable() {
        let err = FcError::Config(ConfigError::InvalidValue {
            field: "config",
            reason: "bad".into(),
        });
        let env = envelope(&err);
        let json = serde_json::to_string(&env).unwrap();
        // Fields variant, detail, exit_code must always be present.
        assert!(json.contains("\"variant\""), "missing 'variant': {json}");
        assert!(json.contains("\"detail\""), "missing 'detail': {json}");
        assert!(
            json.contains("\"exit_code\""),
            "missing 'exit_code': {json}"
        );
    }

    #[test]
    fn rendered_error_json_is_versioned() {
        let err = FcError::Config(ConfigError::InvalidValue {
            field: "config",
            reason: "bad".into(),
        });
        let env = envelope(&err);
        let rendered = json::to_pretty(&env);
        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();

        assert_eq!(parsed["version"], 1);
        assert_eq!(parsed["data"]["variant"], "Config");
        assert_eq!(parsed["data"]["exit_code"], EXIT_CONFIG);
    }
}
