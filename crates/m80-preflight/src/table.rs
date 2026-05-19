//! Fixed-width ASCII table renderer for the preflight report.

use std::fmt::Write as _;

use crate::CheckRow;

/// Column widths (characters).
const LABEL_WIDTH: usize = 24;
const DETAIL_MAX: usize = 60;

/// Render a slice of [`CheckRow`]s as a fixed-width ASCII table.
///
/// Output format (one row per check):
/// ```text
/// +--------+---------------------------+----------------------------------------------+
/// | PASS   | Label                    | Detail text here                             |
/// | FAIL   | Another label            | Failure detail                               |
/// +--------+---------------------------+----------------------------------------------+
/// ```
///
/// Uses plain ASCII only — no Unicode box-drawing — to stay grep-friendly.
pub(crate) fn render(rows: &[CheckRow]) -> String {
    // Separator: +----+------------------------+----...----+
    let sep = format!(
        "+--------+{}+{}+\n",
        "-".repeat(LABEL_WIDTH + 2),
        "-".repeat(DETAIL_MAX + 2),
    );

    let mut out = String::new();
    out.push_str(&sep);
    for row in rows {
        let status = if row.passed { "PASS" } else { "FAIL" };
        // Truncate label to LABEL_WIDTH, left-padded.
        let label_chars: Vec<char> = row.label.chars().collect();
        let label: String = if label_chars.len() > LABEL_WIDTH {
            label_chars[..LABEL_WIDTH].iter().collect()
        } else {
            format!("{:<width$}", row.label, width = LABEL_WIDTH)
        };

        // Wrap detail at DETAIL_MAX chars; first segment on the same line.
        let detail_chars: Vec<char> = row.detail.chars().collect();
        let chunks = char_chunks(&detail_chars, DETAIL_MAX);

        // First line.
        let first = chunks.first().map(String::as_str).unwrap_or("");
        writeln!(
            out,
            "| {status:<6} | {label} | {first:<width$} |",
            width = DETAIL_MAX,
        )
        .unwrap();

        // Continuation lines (overflow detail only, blank status + label).
        for chunk in chunks.iter().skip(1) {
            writeln!(
                out,
                "| {blank:<6} | {blank_label} | {chunk:<width$} |",
                blank = "",
                blank_label = " ".repeat(LABEL_WIDTH),
                width = DETAIL_MAX,
            )
            .unwrap();
        }
    }
    out.push_str(&sep);
    out
}

/// Split a char slice into chunks of at most `width` chars, returning
/// owned `String`s.
fn char_chunks(chars: &[char], width: usize) -> Vec<String> {
    if chars.is_empty() {
        return vec![String::new()];
    }
    chars
        .chunks(width)
        .map(|c| c.iter().collect::<String>())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_rows_produces_two_separators() {
        let out = render(&[]);
        assert_eq!(
            out.lines().count(),
            2,
            "only separator lines for empty input"
        );
    }

    #[test]
    fn pass_row_contains_pass_marker() {
        let rows = vec![CheckRow {
            label: "OS gate".to_string(),
            passed: true,
            detail: "Linux 6.1.0".to_string(),
        }];
        let out = render(&rows);
        assert!(out.contains("PASS"), "PASS must appear for passed row");
        assert!(out.contains("OS gate"), "label must appear");
    }

    #[test]
    fn fail_row_contains_fail_marker() {
        let rows = vec![CheckRow {
            label: "KVM".to_string(),
            passed: false,
            detail: "/dev/kvm not found".to_string(),
        }];
        let out = render(&rows);
        assert!(out.contains("FAIL"), "FAIL must appear for failed row");
    }

    #[test]
    fn long_detail_wraps_to_continuation_line() {
        let long_detail = "x".repeat(DETAIL_MAX + 10);
        let rows = vec![CheckRow {
            label: "Wrap test".to_string(),
            passed: true,
            detail: long_detail,
        }];
        let out = render(&rows);
        // Should have separator + main line + continuation + separator = 4 lines min
        assert!(
            out.lines().count() >= 4,
            "long detail must produce continuation line"
        );
    }
}
