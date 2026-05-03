//! Error → exit-code mapping and JSON error envelope for `m80`.
//!
//! Each `FcError` variant maps to a stable, distinct exit code so callers
//! can branch on error class without parsing stderr text.
//!
//! Behavior captures: bead `m80-4ef.3.1` (exit-code map),
//! `m80-4ef.3.2` (JSON envelope), `m80-4ef.3.3` (stderr informational).

use serde::{Deserialize, Serialize};

use m80_firecracker::FcError;

// =====================================================================
// Exit-code constants (stable per variant — do not renumber)
// =====================================================================

/// Generic / unclassified error.
pub const EXIT_GENERIC: i32 = 1;
/// Preflight check failed.
pub const EXIT_PREFLIGHT: i32 = 2;
/// Admission refused (concurrency limit).
pub const EXIT_ADMISSION: i32 = 3;
/// Manifest read/validate failed.
pub const EXIT_MANIFEST: i32 = 4;
/// Invalid lifecycle state.
pub const EXIT_INVALID_STATE: i32 = 5;
/// Configuration loading or merging failure.
pub const EXIT_CONFIG: i32 = 6;

/// Map an [`FcError`] to its stable CLI exit code.
///
/// The mapping is intentionally one-to-one at the variant level so
/// downstream scripts can `case $?` without parsing stderr.
pub fn exit_code_for(err: &FcError) -> i32 {
    match err {
        FcError::Preflight(_) => EXIT_PREFLIGHT,
        FcError::AdmissionRefused { .. } => EXIT_ADMISSION,
        FcError::Manifest(_) => EXIT_MANIFEST,
        FcError::InvalidState { .. } => EXIT_INVALID_STATE,
        FcError::Config(_) => EXIT_CONFIG,
        // Storage, Jailer, Network, Client, Vsock, Io are all "something
        // went wrong at runtime" — generic.
        FcError::Storage(_)
        | FcError::Jailer(_)
        | FcError::Network(_)
        | FcError::Client(_)
        | FcError::Vsock(_)
        | FcError::Io(_) => EXIT_GENERIC,
    }
}

// =====================================================================
// JSON error envelope (m80-4ef.3.2)
// =====================================================================

/// Machine-readable error envelope emitted on stderr when `--json` is set.
///
/// The `variant` field is the stable Rust variant name callers may match.
/// The `detail` field is the `Display` rendering of the full error chain.
/// The `exit_code` field mirrors the process exit code so callers that
/// capture stderr have both pieces in one object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorEnvelope {
    /// Error class / variant name (e.g., `"Preflight"`, `"Config"`).
    pub variant: &'static str,
    /// Full human-readable description.
    pub detail: String,
    /// Corresponding CLI exit code.
    pub exit_code: i32,
}

/// Build an [`ErrorEnvelope`] from an [`FcError`].
pub fn envelope(err: &FcError) -> ErrorEnvelope {
    ErrorEnvelope {
        variant: variant_name(err),
        detail: err.to_string(),
        exit_code: exit_code_for(err),
    }
}

/// Return the stable variant name string for an [`FcError`].
fn variant_name(err: &FcError) -> &'static str {
    match err {
        FcError::Preflight(_) => "Preflight",
        FcError::Manifest(_) => "Manifest",
        FcError::Storage(_) => "Storage",
        FcError::Jailer(_) => "Jailer",
        FcError::Network(_) => "Network",
        FcError::Client(_) => "Client",
        FcError::Vsock(_) => "Vsock",
        FcError::AdmissionRefused { .. } => "AdmissionRefused",
        FcError::InvalidState { .. } => "InvalidState",
        FcError::Io(_) => "Io",
        FcError::Config(_) => "Config",
    }
}

/// Render the error to stderr (JSON envelope when `json` is true; plain
/// `Display` otherwise) and return the exit code.
///
/// Non-error progress / log lines may already be on stderr (that is fine;
/// stderr is informational — bead m80-4ef.3.3). Only the error itself is
/// written here, always last.
pub fn render_error(err: &FcError, json: bool) -> i32 {
    if json {
        let env = envelope(err);
        // Unwrap: serializing a struct of strings cannot fail.
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&env).expect("error envelope serialization")
        );
    } else {
        eprintln!("error: {err}");
    }
    exit_code_for(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Each variant must produce a distinct exit code that is non-zero.
    #[test]
    fn preflight_is_2() {
        use m80_preflight::PreflightError;
        let err = FcError::Preflight(PreflightError::KvmUnavailable);
        assert_eq!(exit_code_for(&err), EXIT_PREFLIGHT);
    }

    #[test]
    fn admission_refused_is_3() {
        let err = FcError::AdmissionRefused { limit: 4 };
        assert_eq!(exit_code_for(&err), EXIT_ADMISSION);
    }

    #[test]
    fn config_is_6() {
        let err = FcError::Config("bad toml".into());
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
    fn all_variants_are_nonzero() {
        use std::io;
        use m80_preflight::PreflightError;
        let cases: Vec<FcError> = vec![
            FcError::Preflight(PreflightError::KvmUnavailable),
            FcError::AdmissionRefused { limit: 1 },
            FcError::Config("x".into()),
            FcError::InvalidState {
                expected: "a",
                actual: "b",
            },
            FcError::Io(io::Error::new(io::ErrorKind::Other, "test")),
        ];
        for err in &cases {
            assert_ne!(exit_code_for(err), 0, "exit code must be non-zero for {err:?}");
        }
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
        ];
        let mut seen = std::collections::HashSet::new();
        for code in &defined {
            assert!(seen.insert(code), "duplicate exit code: {code}");
        }
    }

    #[test]
    fn json_envelope_fields() {
        let err = FcError::Config("bad".into());
        let env = envelope(&err);
        assert_eq!(env.variant, "Config");
        assert_eq!(env.exit_code, EXIT_CONFIG);
        assert!(!env.detail.is_empty());
    }

    #[test]
    fn json_envelope_is_stable() {
        let err = FcError::Config("bad".into());
        let env = envelope(&err);
        let json = serde_json::to_string(&env).unwrap();
        // Fields variant, detail, exit_code must always be present.
        assert!(json.contains("\"variant\""), "missing 'variant': {json}");
        assert!(json.contains("\"detail\""), "missing 'detail': {json}");
        assert!(json.contains("\"exit_code\""), "missing 'exit_code': {json}");
    }
}
