//! Runtime wire-level dump gated by `M80_DEBUG_WIRE`.
//!
//! Set `M80_DEBUG_WIRE=fcrest` (or `M80_DEBUG_WIRE=all`) to have every
//! Firecracker REST request and response logged via `tracing::trace!`. Output
//! format:
//!
//! ```text
//! direction=out method=PUT path=/boot-source len=123 head_hex=… head_ascii="…"
//! direction=in  status=204 len=0 head_hex= head_ascii=""
//! ```
//!
//! Matching is exact (`==`); unknown tokens are silently ignored. Whitespace
//! inside the value is not trimmed — `M80_DEBUG_WIRE= fcrest` does **not**
//! match; `M80_DEBUG_WIRE=fcrest` does.

use std::collections::HashSet;
use std::sync::OnceLock;

static ENABLED_TARGETS: OnceLock<HashSet<String>> = OnceLock::new();

/// Parse the comma-separated `M80_DEBUG_WIRE` value into enabled targets.
///
/// Parameterised on `&str` so tests can call it without touching the env.
pub(crate) fn parse_targets(raw: &str) -> HashSet<String> {
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
    const CAP: usize = 1024;
    let total = bytes.len();
    let head = &bytes[..total.min(CAP)];
    let head_hex = hex::encode(head);
    let head_ascii: String = head
        .iter()
        .map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' })
        .collect();
    if total > CAP {
        format!(
            "len={total} head_hex={head_hex} head_ascii=\"{head_ascii}\" (truncated)"
        )
    } else {
        format!("len={total} head_hex={head_hex} head_ascii=\"{head_ascii}\"")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parser tests ---

    #[test]
    fn parse_empty_string_yields_one_empty_token() {
        // `"".split(',')` yields one empty token; `is_enabled` then matches
        // nothing real because no live target is the empty string.
        let s = parse_targets("");
        assert!(s.contains(""));
    }

    #[test]
    fn parse_single_fcrest_token() {
        let s = parse_targets("fcrest");
        assert!(s.contains("fcrest"));
        assert!(!s.contains("vsock"));
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
        let s = parse_targets("unknown_token");
        assert!(s.contains("unknown_token"));
        assert!(!s.contains("fcrest"));
    }

    #[test]
    fn parse_malformed_with_spaces_not_trimmed() {
        let s = parse_targets(" fcrest");
        assert!(!s.contains("fcrest"), "space-prefixed token must not match fcrest");
        assert!(s.contains(" fcrest"));
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
}
