//! Runtime wire-level dump gated by `M80_DEBUG_WIRE`.
//!
//! Set `M80_DEBUG_WIRE=vsock` (or `M80_DEBUG_WIRE=all`) to have every
//! vsock frame logged via `tracing::trace!`. Output format:
//!
//! ```text
//! direction=out len=1234 head_hex=7b2276657273696f6e22 head_ascii="{\"version\"" (truncated)
//! ```
//!
//! Matching is exact (`==`); unknown tokens are silently ignored. Whitespace
//! inside the value is not trimmed — `M80_DEBUG_WIRE= vsock` does **not**
//! match; `M80_DEBUG_WIRE=vsock` does.

use std::collections::HashSet;
use std::sync::OnceLock;

use m80_proto::{encode_raw_envelope, wire::WirePayload, RawEnvelope, PAYLOAD_KIND_EXEC_REQUEST};

static ENABLED_TARGETS: OnceLock<HashSet<String>> = OnceLock::new();

/// Parse the comma-separated `M80_DEBUG_WIRE` value into enabled targets.
///
/// Parameterised on `&str` so tests can call it without touching the env.
fn parse_targets(raw: &str) -> HashSet<String> {
    raw.split(',').map(|t| t.to_owned()).collect()
}

fn targets() -> &'static HashSet<String> {
    ENABLED_TARGETS.get_or_init(|| {
        std::env::var("M80_DEBUG_WIRE")
            .map(|v| parse_targets(&v))
            .unwrap_or_default()
    })
}

/// Return `true` if wire-level logging is enabled for `target`.
pub(crate) fn is_enabled(target: &'static str) -> bool {
    let t = targets();
    t.contains("all") || t.contains(target)
}

/// Format up to 1024 bytes of `bytes` as a hex+ASCII preview string.
///
/// Output: `len=N head_hex=… head_ascii="…"`, with `(truncated)` appended
/// when the payload exceeds 1024 bytes.
pub(crate) fn format_wire_preview(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    const CAP: usize = 1024;
    let total = bytes.len();
    let head = &bytes[..total.min(CAP)];
    // Capacity: "len=" prefix + digits + " head_hex=" + 2*head + " head_ascii=\"" + head + "\"" + optional " (truncated)"
    let mut out = String::with_capacity(32 + head.len() * 3 + 16);
    write!(out, "len={total} head_hex=").unwrap();
    for b in head {
        write!(out, "{b:02x}").unwrap();
    }
    out.push_str(" head_ascii=\"");
    for &b in head {
        out.push(if b.is_ascii_graphic() || b == b' ' {
            b as char
        } else {
            '.'
        });
    }
    out.push('"');
    if total > CAP {
        out.push_str(" (truncated)");
    }
    out
}

/// Format a raw envelope preview without exposing `ExecRequest.env` values.
///
/// The caller still sends the original envelope. This function clones only
/// the debug copy, clears env entries on exec requests, and appends a stable
/// redaction marker with the number of removed entries.
pub(crate) fn format_envelope_preview(raw: &RawEnvelope) -> Result<String, m80_proto::ProtoError> {
    let mut redacted = raw.clone();
    let env_count = redact_exec_request_env(&mut redacted);
    let bytes = encode_raw_envelope(redacted)?;
    let mut preview = format_wire_preview(&bytes);
    if let Some(count) = env_count {
        use std::fmt::Write as _;
        write!(preview, " env=[{count} entries redacted]").unwrap();
    }
    Ok(preview)
}

fn redact_exec_request_env(raw: &mut RawEnvelope) -> Option<usize> {
    if raw.kind != PAYLOAD_KIND_EXEC_REQUEST {
        return None;
    }
    let WirePayload::ExecRequest(request) = &mut raw.payload else {
        return None;
    };
    let count = request.env.len();
    request.env.clear();
    Some(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use m80_proto::{Envelope, ExecRequest};

    // --- parser tests ---

    #[test]
    fn parse_empty_string_yields_one_empty_token() {
        // `"".split(',')` yields one empty token; `is_enabled` then matches
        // nothing real because no live target is the empty string.
        let s = parse_targets("");
        assert!(s.contains(""));
    }

    #[test]
    fn parse_single_vsock_token() {
        let s = parse_targets("vsock");
        assert!(s.contains("vsock"));
        assert!(!s.contains("fcrest"));
        assert!(!s.contains("all"));
    }

    #[test]
    fn parse_vsock_and_fcrest() {
        let s = parse_targets("vsock,fcrest");
        assert!(s.contains("vsock"));
        assert!(s.contains("fcrest"));
    }

    #[test]
    fn parse_all_token() {
        let s = parse_targets("all");
        assert!(s.contains("all"));
    }

    #[test]
    fn parse_unknown_token_included_in_set() {
        // Unknown tokens are not errors; they simply don't match known targets.
        let s = parse_targets("unknown_token");
        assert!(s.contains("unknown_token"));
        assert!(!s.contains("vsock"));
    }

    #[test]
    fn parse_malformed_with_spaces_not_trimmed() {
        // Spaces are NOT trimmed — " vsock" is a distinct token from "vsock".
        let s = parse_targets(" vsock");
        assert!(
            !s.contains("vsock"),
            "space-prefixed token must not match vsock"
        );
        assert!(s.contains(" vsock"));
    }

    // --- format_wire_preview tests ---

    #[test]
    fn preview_short_input_no_truncation_marker() {
        let bytes = b"hello";
        let out = format_wire_preview(bytes);
        assert!(out.starts_with("len=5 "));
        assert!(!out.contains("(truncated)"));
    }

    #[test]
    fn preview_exactly_1024_bytes_no_truncation_marker() {
        let bytes = vec![b'a'; 1024];
        let out = format_wire_preview(&bytes);
        assert!(out.starts_with("len=1024 "));
        assert!(!out.contains("(truncated)"));
    }

    #[test]
    fn preview_over_1024_bytes_has_truncation_marker() {
        let bytes = vec![b'b'; 1025];
        let out = format_wire_preview(&bytes);
        assert!(out.starts_with("len=1025 "));
        assert!(out.contains("(truncated)"));
    }

    #[test]
    fn exec_request_preview_redacts_env_values() {
        let envelope = Envelope::new(ExecRequest {
            program: "/bin/sh".to_owned(),
            args: vec!["-lc".to_owned(), "true".to_owned()],
            cwd: None,
            env: Some(vec![(
                "API_KEY".to_owned(),
                "sentinel-secret-for-debug-preview".to_owned(),
            )]),
            stdin: None,
            timeout_ms: None,
            streaming: false,
        });

        let preview =
            format_envelope_preview(&RawEnvelope::from_typed(&envelope)).expect("format preview");

        assert!(!preview.contains("sentinel-secret-for-debug-preview"));
        assert!(preview.contains("env=[1 entries redacted]"));
    }
}
