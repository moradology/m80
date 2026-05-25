//! Error → exit-code mapping and JSON error envelope for `m80`.
//!
//! Each `FcError` variant maps to a stable, distinct exit code so callers
//! can branch on error class without parsing stderr text.
//!
//! Behavior captures: bead `m80-4ef.3.1` (exit-code map),
//! `m80-4ef.3.2` (JSON envelope), `m80-4ef.3.3` (stderr informational).

use serde::{Deserialize, Serialize};

use m80_firecracker::{ConfigError, FcError};
use m80_preflight::HostPrerequisiteCheck;

use crate::json;
use crate::request_id;

pub(crate) fn host_io(operation: &'static str, source: std::io::Error) -> FcError {
    FcError::HostIo { operation, source }
}

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
/// Generic timeout.
pub(crate) const EXIT_TIMEOUT: i32 = 9;
/// Run-dir ownership conflict or not found.
pub(crate) const EXIT_RUN_DIR_OWNERSHIP: i32 = 10;
/// VM idle watchdog fired; session timed out.
pub(crate) const EXIT_IDLE_TIMED_OUT: i32 = 11;
/// VM wall-clock lifetime cap fired.
pub(crate) const EXIT_LIFETIME_EXPIRED: i32 = EXIT_TIMEOUT;
/// One-shot VM was already consumed; cannot reuse.
pub(crate) const EXIT_ONE_SHOT_CONSUMED: i32 = 12;
/// Recorded Firecracker process is dead before a new request.
pub(crate) const EXIT_SANDBOX_DEAD: i32 = 13;
/// Host infrastructure failed before guest readiness.
pub(crate) const EXIT_HOST_INFRASTRUCTURE: i32 = 14;
/// VM booted far enough for Firecracker but guestd never became ready.
pub(crate) const EXIT_GUESTD_READY: i32 = 15;

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
        FcError::Config(_) | FcError::InvalidVmId { .. } => EXIT_CONFIG,
        FcError::UnsupportedOperation { .. } => EXIT_NOT_IMPLEMENTED,
        FcError::ExecTimeoutHost { .. } => EXIT_TIMEOUT,
        FcError::CleanupDeadlineExceeded { .. } => EXIT_TIMEOUT,
        FcError::ApiSocketTimeout { .. } | FcError::HostInfrastructure { .. } => {
            EXIT_HOST_INFRASTRUCTURE
        }
        FcError::GuestdReadyTimeout { .. } => EXIT_GUESTD_READY,
        FcError::RunDirOwnershipAmbiguous { .. }
        | FcError::RunDirAlreadyOwned { .. }
        | FcError::RunDirNotFound { .. }
        | FcError::StaleCgroupLeaf { .. } => EXIT_RUN_DIR_OWNERSHIP,
        FcError::IdleTimedOut => EXIT_IDLE_TIMED_OUT,
        FcError::LifetimeExpired { .. } => EXIT_LIFETIME_EXPIRED,
        FcError::OneShotConsumed => EXIT_ONE_SHOT_CONSUMED,
        FcError::SandboxDead { .. } => EXIT_SANDBOX_DEAD,
        // Storage, Jailer, Network, Client, Vsock, host I/O, and typed runtime
        // cleanup/serialization failures are all "something went wrong at
        // runtime" — generic.
        FcError::Storage(_)
        | FcError::ImageStore(_)
        | FcError::TemplateStore(_)
        | FcError::Jailer(_)
        | FcError::Cgroup(_)
        | FcError::Network(_)
        | FcError::NetworkHelper(_)
        | FcError::CapabilityDrop(_)
        | FcError::Client(_)
        | FcError::Vsock(_)
        | FcError::Protocol(_)
        | FcError::Snapshot(_)
        | FcError::FileOp(_)
        | FcError::DriveHotplug(_)
        | FcError::PmemMount(_)
        | FcError::PostRestoreHook(_)
        | FcError::TenantIdentityMismatch { .. }
        | FcError::FileUploadReadFailed { .. }
        | FcError::HostIo { .. }
        | FcError::PathIo { .. }
        | FcError::Json { .. }
        | FcError::CommandSpawnFailed { .. }
        | FcError::CommandFailed { .. }
        | FcError::ArtifactMissing { .. }
        | FcError::WarmPoolFillFailed { .. }
        | FcError::WarmReadyProbeRejected { .. }
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
    /// Optional stable diagnostic code for contract-level failures that share
    /// a broader `FcError` variant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) code: Option<ErrorDiagnosticCode>,
    /// Full human-readable description.
    pub(crate) detail: String,
    /// Corresponding CLI exit code.
    pub(crate) exit_code: i32,
    /// Number of warm slots that will be ready, when the error is `PoolEmpty`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) target_ready: Option<usize>,
    /// Structured host-prerequisite failure for preflight errors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) host_prerequisite_failure: Option<HostPrerequisiteCheck>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ErrorDiagnosticCode {
    DirectUrlClassifier,
    ReleaseMaterialFetch,
    ReleaseMaterialDigest,
    ReleaseMaterialAttestation,
    ReleaseMaterialStale,
    InstallNoWriteRollback,
    NoInstalledProfile,
}

/// Build an [`ErrorEnvelope`] from an [`FcError`].
pub(crate) fn envelope(err: &FcError) -> ErrorEnvelope {
    let target_ready = match err {
        FcError::PoolEmpty { target_ready } => Some(*target_ready),
        _ => None,
    };
    ErrorEnvelope {
        variant: err.variant_name(),
        code: diagnostic_code_for(err),
        detail: err.to_string(),
        exit_code: exit_code_for(err),
        target_ready,
        host_prerequisite_failure: match err {
            FcError::Preflight(preflight) => HostPrerequisiteCheck::from_preflight_error(preflight),
            _ => None,
        },
    }
}

fn diagnostic_code_for(err: &FcError) -> Option<ErrorDiagnosticCode> {
    match err {
        FcError::Config(ConfigError::InvalidValue { field, reason }) => match *field {
            "default_profile" if reason.contains("no installed default profile") => {
                Some(ErrorDiagnosticCode::NoInstalledProfile)
            }
            "bundle-url" => Some(ErrorDiagnosticCode::DirectUrlClassifier),
            "bundle-url.sha256" => Some(ErrorDiagnosticCode::ReleaseMaterialDigest),
            "attestation-verifier" => Some(ErrorDiagnosticCode::ReleaseMaterialAttestation),
            "release-material" => Some(release_material_diagnostic_code(reason)),
            "install.finalization" | "install.no_write_rollback" => {
                Some(ErrorDiagnosticCode::InstallNoWriteRollback)
            }
            _ => None,
        },
        FcError::UnsupportedOperation {
            operation: "m80 install",
            reason,
        } if reason.contains("bundle URL") => Some(ErrorDiagnosticCode::DirectUrlClassifier),
        _ => None,
    }
}

fn release_material_diagnostic_code(reason: &str) -> ErrorDiagnosticCode {
    let reason = reason.to_ascii_lowercase();
    if reason.contains("fetch failed")
        || reason.contains("fetch spawn failed")
        || reason.contains("asset index fetch failed")
    {
        return ErrorDiagnosticCode::ReleaseMaterialFetch;
    }
    if reason.contains("attestation") || reason.contains("gh_bin") {
        return ErrorDiagnosticCode::ReleaseMaterialAttestation;
    }
    if reason.contains("repository mismatch")
        || reason.contains("release_tag mismatch")
        || reason.contains("target mismatch")
        || reason.contains("m80_version")
        || reason.contains("schema_version")
        || reason.contains("mechanism mismatch")
        || reason.contains("source_ref")
        || reason.contains("subject set mismatch")
        || reason.contains("unexpected subject")
    {
        return ErrorDiagnosticCode::ReleaseMaterialStale;
    }
    if reason.contains("sha256") || reason.contains("checksum") || reason.contains("digest") {
        return ErrorDiagnosticCode::ReleaseMaterialDigest;
    }
    ErrorDiagnosticCode::ReleaseMaterialStale
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
    use m80_firecracker::{ConfigError, HostFaultKind};

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
        assert_eq!(err.variant_name(), "PoolEmpty");
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
    fn invalid_vm_id_is_6_with_named_variant() {
        let err = FcError::InvalidVmId {
            vm_id: "..".into(),
            reason: "must not be a traversal component".into(),
        };
        assert_eq!(exit_code_for(&err), EXIT_CONFIG);
        assert_eq!(envelope(&err).variant, "InvalidVmId");
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
    fn host_io_is_generic() {
        use std::io;
        let err = FcError::HostIo {
            operation: "test",
            source: io::Error::new(io::ErrorKind::NotFound, "gone"),
        };
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
    fn preflight_envelope_carries_host_prerequisite_repair_token() {
        use m80_preflight::PreflightError;
        let err = FcError::Preflight(PreflightError::KvmUnavailable {
            path: "/dev/kvm".into(),
        });

        let env = envelope(&err);

        let failure = env
            .host_prerequisite_failure
            .expect("preflight envelope should carry host prerequisite failure");
        assert_eq!(
            failure.check_id,
            m80_preflight::HostPrerequisiteCheckId::Kvm
        );
        assert_eq!(
            failure.remediation.as_ref().unwrap().id,
            m80_preflight::REPAIR_KVM
        );
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
    fn host_io_exit_code_is_nonzero() {
        use std::io;
        let err = FcError::HostIo {
            operation: "test",
            source: io::Error::other("test"),
        };
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
            EXIT_HOST_INFRASTRUCTURE,
            EXIT_GUESTD_READY,
            EXIT_ONE_SHOT_CONSUMED,
            EXIT_SANDBOX_DEAD,
        ];
        let mut seen = std::collections::HashSet::new();
        for code in &defined {
            assert!(seen.insert(code), "duplicate exit code: {code}");
        }
    }

    #[test]
    fn host_infrastructure_is_14() {
        let err = FcError::HostInfrastructure {
            kind: HostFaultKind::ApiSocketTimeout,
            detail: "api socket did not appear".into(),
        };
        let env = envelope(&err);
        assert_eq!(exit_code_for(&err), EXIT_HOST_INFRASTRUCTURE);
        assert_eq!(env.variant, "HostInfrastructure");
        assert_eq!(env.exit_code, EXIT_HOST_INFRASTRUCTURE);
    }

    #[test]
    fn api_socket_timeout_is_host_infrastructure() {
        let err = FcError::ApiSocketTimeout {
            path: "/run/m80/firecracker.sock".into(),
            timeout: std::time::Duration::from_secs(5),
        };
        let env = envelope(&err);
        assert_eq!(exit_code_for(&err), EXIT_HOST_INFRASTRUCTURE);
        assert_eq!(env.variant, "ApiSocketTimeout");
        assert_eq!(env.exit_code, EXIT_HOST_INFRASTRUCTURE);
    }

    #[test]
    fn guestd_ready_timeout_is_15() {
        let err = FcError::GuestdReadyTimeout {
            path: "/run/m80/vsock.sock_9000".into(),
            timeout: std::time::Duration::from_secs(60),
        };
        let env = envelope(&err);
        assert_eq!(exit_code_for(&err), EXIT_GUESTD_READY);
        assert_eq!(env.variant, "GuestdReadyTimeout");
        assert_eq!(env.exit_code, EXIT_GUESTD_READY);
    }

    #[test]
    fn host_exec_timeout_is_9() {
        let err = FcError::ExecTimeoutHost {
            timeout: std::time::Duration::from_secs(5),
        };
        assert_eq!(exit_code_for(&err), EXIT_TIMEOUT);
        assert_eq!(envelope(&err).variant, "ExecTimeoutHost");
    }

    #[test]
    fn run_dir_already_owned_is_10() {
        let err = FcError::RunDirAlreadyOwned {
            run_dir: "/run/m80/x".into(),
            pid: 1234,
        };
        assert_eq!(exit_code_for(&err), EXIT_RUN_DIR_OWNERSHIP);
    }

    #[test]
    fn run_dir_ownership_ambiguous_is_10() {
        let err = FcError::RunDirOwnershipAmbiguous {
            run_dir: "/run/m80/x".into(),
        };
        assert_eq!(exit_code_for(&err), EXIT_RUN_DIR_OWNERSHIP);
    }

    #[test]
    fn run_dir_not_found_is_10() {
        let err = FcError::RunDirNotFound {
            vm_id: "vm0".into(),
            run_dir: "/run/m80/x".into(),
        };
        assert_eq!(exit_code_for(&err), EXIT_RUN_DIR_OWNERSHIP);
    }

    #[test]
    fn stale_cgroup_leaf_is_10() {
        let err = FcError::StaleCgroupLeaf {
            vm_id: "vm0".into(),
            path: "/sys/fs/cgroup/m80-firecracker/vm0".into(),
            pids: "4242".into(),
        };
        assert_eq!(exit_code_for(&err), EXIT_RUN_DIR_OWNERSHIP);
        assert_eq!(envelope(&err).variant, "StaleCgroupLeaf");
    }

    #[test]
    fn idle_timed_out_is_11() {
        assert_eq!(exit_code_for(&FcError::IdleTimedOut), EXIT_IDLE_TIMED_OUT);
    }

    #[test]
    fn lifetime_expired_is_9() {
        let err = FcError::LifetimeExpired {
            limit: std::time::Duration::from_secs(60),
        };
        assert_eq!(exit_code_for(&err), EXIT_LIFETIME_EXPIRED);
        assert_eq!(envelope(&err).variant, "LifetimeExpired");
    }

    #[test]
    fn one_shot_consumed_is_12() {
        assert_eq!(
            exit_code_for(&FcError::OneShotConsumed),
            EXIT_ONE_SHOT_CONSUMED
        );
    }

    #[test]
    fn sandbox_dead_is_13() {
        let err = FcError::SandboxDead {
            vm_id: "vm0".into(),
            firecracker_pid: 1234,
        };
        assert_eq!(exit_code_for(&err), EXIT_SANDBOX_DEAD);
        assert_eq!(envelope(&err).variant, "SandboxDead");
    }

    #[test]
    fn json_envelope_fields() {
        let err = FcError::Config(ConfigError::InvalidValue {
            field: "config",
            reason: "bad".into(),
        });
        let env = envelope(&err);
        assert_eq!(env.variant, "Config");
        assert_eq!(env.code, None);
        assert_eq!(env.exit_code, EXIT_CONFIG);
        assert!(!env.detail.is_empty());
    }

    #[test]
    fn json_envelope_codes_direct_url_diagnostics() {
        for (err, expected) in [
            (
                FcError::Config(ConfigError::InvalidValue {
                    field: "bundle-url",
                    reason: "remote bundle URL must be a concrete moradology/m80 asset".into(),
                }),
                ErrorDiagnosticCode::DirectUrlClassifier,
            ),
            (
                FcError::Config(ConfigError::InvalidValue {
                    field: "release-material",
                    reason: "release material fetch failed: release_tag=v0.0.0 material_class=install-script".into(),
                }),
                ErrorDiagnosticCode::ReleaseMaterialFetch,
            ),
            (
                FcError::Config(ConfigError::InvalidValue {
                    field: "release-material",
                    reason: "release material checksum mismatch: material_class=bundle-checksum".into(),
                }),
                ErrorDiagnosticCode::ReleaseMaterialDigest,
            ),
            (
                FcError::Config(ConfigError::InvalidValue {
                    field: "release-material",
                    reason: "release trust native attestation bundle verification failed: material_class=release-attestation-bundle".into(),
                }),
                ErrorDiagnosticCode::ReleaseMaterialAttestation,
            ),
            (
                FcError::Config(ConfigError::InvalidValue {
                    field: "release-material",
                    reason: "release integrity release_tag mismatch: material_class=release-integrity-predicate expected v0.0.0, got v9.9.9".into(),
                }),
                ErrorDiagnosticCode::ReleaseMaterialStale,
            ),
            (
                FcError::Config(ConfigError::InvalidValue {
                    field: "install.finalization",
                    reason: "injected interruption after profile write".into(),
                }),
                ErrorDiagnosticCode::InstallNoWriteRollback,
            ),
        ] {
            assert_eq!(envelope(&err).code, Some(expected), "{err}");
        }
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
