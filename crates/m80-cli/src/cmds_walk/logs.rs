use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use m80_firecracker::{FcError, CONSOLE_LOG};
use m80_observability::DIAGNOSTICS_FILE_NAME;
use serde::Serialize;

use crate::errors;
use crate::json;

#[derive(Debug, Clone, Serialize)]
struct LogsOutput {
    version: u16,
    vm_id: String,
    run_dir: PathBuf,
    records: Vec<LogRecord>,
}

#[derive(Debug, Clone, Serialize)]
struct LogRecord {
    source: LogSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    timestamp_unix_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    level: Option<String>,
    message: String,
    raw: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum LogSource {
    Host,
    Guest,
}

#[derive(Debug, Clone, Copy)]
struct LogFilter<'a> {
    request_id: Option<&'a str>,
    since_unix_ms: Option<u64>,
}

#[derive(Default)]
struct FollowCursor {
    diagnostics_lines: usize,
    console_lines: usize,
}

pub(crate) fn cmd_logs(
    vm_id: &str,
    follow: bool,
    request_id: Option<&str>,
    since: Option<&str>,
    json_mode: bool,
) -> anyhow::Result<i32> {
    let run_root = match super::effective_run_root() {
        Ok(run_root) => run_root,
        Err(e) => return Ok(errors::render_error(&e, json_mode)),
    };
    let filter = match parse_filter(request_id, since) {
        Ok(filter) => filter,
        Err(e) => return Ok(errors::render_error(&e, json_mode)),
    };
    let run_dir = run_root.join(vm_id);
    if !run_dir.exists() {
        let e = FcError::RunDirNotFound {
            vm_id: vm_id.to_owned(),
            run_dir,
        };
        return Ok(errors::render_error(&e, json_mode));
    }

    let mut cursor = FollowCursor::default();
    let output = match logs_output(&run_dir, vm_id, filter, &mut cursor) {
        Ok(output) => output,
        Err(e) => return Ok(errors::render_error(&e, json_mode)),
    };
    emit_output(&output, json_mode);

    if follow {
        loop {
            std::thread::sleep(Duration::from_millis(500));
            let output = match logs_output(&run_dir, vm_id, filter, &mut cursor) {
                Ok(output) => output,
                Err(e) => return Ok(errors::render_error(&e, json_mode)),
            };
            if output.records.is_empty() {
                continue;
            }
            emit_output(&output, json_mode);
        }
    }

    Ok(0)
}

fn parse_filter<'a>(
    request_id: Option<&'a str>,
    since: Option<&str>,
) -> Result<LogFilter<'a>, FcError> {
    let since_unix_ms = since.map(parse_since).transpose()?;
    Ok(LogFilter {
        request_id,
        since_unix_ms,
    })
}

fn logs_output(
    run_dir: &Path,
    vm_id: &str,
    filter: LogFilter<'_>,
    cursor: &mut FollowCursor,
) -> Result<LogsOutput, FcError> {
    let mut records = Vec::new();
    records.extend(read_diagnostics_records(run_dir, filter, cursor)?);
    records.extend(read_console_records(run_dir, filter, cursor)?);
    records.sort_by_key(|record| record.timestamp_unix_ms.unwrap_or(u64::MAX));
    Ok(LogsOutput {
        version: 1,
        vm_id: vm_id.to_owned(),
        run_dir: run_dir.to_path_buf(),
        records,
    })
}

fn read_diagnostics_records(
    run_dir: &Path,
    filter: LogFilter<'_>,
    cursor: &mut FollowCursor,
) -> Result<Vec<LogRecord>, FcError> {
    let path = run_dir.join(DIAGNOSTICS_FILE_NAME);
    let lines = read_lines_if_present(&path)?;
    let mut records = Vec::new();
    for line in lines.iter().skip(cursor.diagnostics_lines) {
        if let Some(record) = diagnostics_record(line, filter) {
            records.push(record);
        }
    }
    cursor.diagnostics_lines = lines.len();
    Ok(records)
}

fn read_console_records(
    run_dir: &Path,
    filter: LogFilter<'_>,
    cursor: &mut FollowCursor,
) -> Result<Vec<LogRecord>, FcError> {
    let path = run_dir.join(CONSOLE_LOG);
    let lines = read_lines_if_present(&path)?;
    let mut records = Vec::new();
    for line in lines.iter().skip(cursor.console_lines) {
        if let Some(record) = console_record(line, filter) {
            records.push(record);
        }
    }
    cursor.console_lines = lines.len();
    Ok(records)
}

fn read_lines_if_present(path: &Path) -> Result<Vec<String>, FcError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text.lines().map(str::to_owned).collect()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(source) => Err(FcError::PathIo {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Partial projection of a diagnostics JSONL record. Intentionally omits
/// `#[serde(deny_unknown_fields)]` — the real record carries more fields
/// (context, level, etc.) and we only need these five for filtering/rendering.
#[derive(serde::Deserialize)]
struct DiagnosticsRecord {
    timestamp_unix_ms: Option<u64>,
    request_id: Option<String>,
    phase: Option<String>,
    event_kind: Option<String>,
    #[serde(default)]
    message: String,
}

fn diagnostics_record(line: &str, filter: LogFilter<'_>) -> Option<LogRecord> {
    let rec: DiagnosticsRecord = match serde_json::from_str(line) {
        Ok(rec) => rec,
        Err(e) => {
            eprintln!("m80 logs: malformed JSONL line ({e}); raw: {line:?}");
            return None;
        }
    };
    if !passes_filter(rec.request_id.as_deref(), rec.timestamp_unix_ms, filter) {
        return None;
    }
    Some(LogRecord {
        source: LogSource::Host,
        timestamp_unix_ms: rec.timestamp_unix_ms,
        timestamp: None,
        request_id: rec.request_id,
        phase: rec.phase,
        level: rec.event_kind,
        message: rec.message,
        raw: line.to_owned(),
    })
}

fn console_record(line: &str, filter: LogFilter<'_>) -> Option<LogRecord> {
    let parsed = parse_guest_line(line);
    if !passes_filter(
        parsed.request_id.as_deref(),
        parsed.timestamp_unix_ms,
        filter,
    ) {
        return None;
    }
    Some(LogRecord {
        source: LogSource::Guest,
        timestamp_unix_ms: parsed.timestamp_unix_ms,
        timestamp: parsed.timestamp,
        request_id: parsed.request_id,
        phase: parsed.phase,
        level: parsed.level,
        message: parsed.message,
        raw: line.to_owned(),
    })
}

struct ParsedGuestLine {
    timestamp_unix_ms: Option<u64>,
    timestamp: Option<String>,
    request_id: Option<String>,
    phase: Option<String>,
    level: Option<String>,
    message: String,
}

fn parse_guest_line(line: &str) -> ParsedGuestLine {
    if let Some((timestamp, rest)) = bracketed(line) {
        if let Some((phase, rest)) = bracketed(rest.trim_start()) {
            if let Some((request_id, rest)) = bracketed(rest.trim_start()) {
                let rest = rest.trim_start();
                let (level, message) = rest
                    .split_once(' ')
                    .map(|(level, message)| (Some(level.to_owned()), message.to_owned()))
                    .unwrap_or((None, rest.to_owned()));
                return ParsedGuestLine {
                    timestamp_unix_ms: rfc3339_seconds_to_unix_ms(timestamp),
                    timestamp: Some(timestamp.to_owned()),
                    request_id: (request_id != "boot").then(|| request_id.to_owned()),
                    phase: Some(phase.to_owned()),
                    level,
                    message,
                };
            }
        }
    }
    ParsedGuestLine {
        timestamp_unix_ms: None,
        timestamp: None,
        request_id: None,
        phase: None,
        level: None,
        message: line.to_owned(),
    }
}

fn bracketed(input: &str) -> Option<(&str, &str)> {
    let rest = input.strip_prefix('[')?;
    let (value, rest) = rest.split_once(']')?;
    Some((value, rest))
}

fn passes_filter(
    request_id: Option<&str>,
    timestamp_unix_ms: Option<u64>,
    filter: LogFilter<'_>,
) -> bool {
    if let Some(expected) = filter.request_id {
        if request_id != Some(expected) {
            return false;
        }
    }
    if let Some(since) = filter.since_unix_ms {
        if timestamp_unix_ms.is_none_or(|timestamp| timestamp < since) {
            return false;
        }
    }
    true
}

fn parse_since(value: &str) -> Result<u64, FcError> {
    if let Ok(ms) = value.parse::<u64>() {
        return Ok(ms);
    }
    rfc3339_seconds_to_unix_ms(value).ok_or_else(|| {
        FcError::Config(m80_firecracker::ConfigError::InvalidValue {
            field: "since",
            reason: format!("must be UNIX milliseconds or YYYY-MM-DDTHH:MM:SSZ, got `{value}`"),
        })
    })
}

fn rfc3339_seconds_to_unix_ms(value: &str) -> Option<u64> {
    let year = value.get(0..4)?.parse::<i32>().ok()?;
    if value.get(4..5)? != "-"
        || value.get(7..8)? != "-"
        || value.get(10..11)? != "T"
        || value.get(13..14)? != ":"
        || value.get(16..17)? != ":"
        || value.get(19..20)? != "Z"
    {
        return None;
    }
    let month = value.get(5..7)?.parse::<u32>().ok()?;
    let day = value.get(8..10)?.parse::<u32>().ok()?;
    let hour = value.get(11..13)?.parse::<u64>().ok()?;
    let minute = value.get(14..16)?.parse::<u64>().ok()?;
    let second = value.get(17..19)?.parse::<u64>().ok()?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let days = days_from_civil(year, month, day)?;
    let secs = days
        .checked_mul(86_400)?
        .checked_add(hour.checked_mul(3_600)?)?
        .checked_add(minute.checked_mul(60)?)?
        .checked_add(second)?;
    secs.checked_mul(1_000)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<u64> {
    let year = year as i64 - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month = month as i64;
    let day = day as i64;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    u64::try_from(days).ok()
}

fn emit_output(output: &LogsOutput, json_mode: bool) {
    if json_mode {
        println!("{}", json::to_pretty(output));
    } else {
        print!("{}", render_logs_human(output));
    }
}

fn render_logs_human(output: &LogsOutput) -> String {
    let mut out = String::new();
    for record in &output.records {
        let source = match record.source {
            LogSource::Host => "host",
            LogSource::Guest => "guest",
        };
        let timestamp = record
            .timestamp
            .clone()
            .or_else(|| record.timestamp_unix_ms.map(|ms| ms.to_string()))
            .unwrap_or_else(|| "-".to_owned());
        let phase = record.phase.as_deref().unwrap_or("-");
        let request_id = record.request_id.as_deref().unwrap_or("-");
        let level = record.level.as_deref().unwrap_or("-");
        writeln!(
            out,
            "[{source}] {timestamp} {phase} {request_id} {level} {}",
            record.message
        )
        .unwrap();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_logs(run_dir: &Path) {
        std::fs::create_dir_all(run_dir).unwrap();
        std::fs::write(
            run_dir.join(DIAGNOSTICS_FILE_NAME),
            r#"{"schema_version":2,"timestamp_unix_ms":2000,"event_kind":"lifecycle","source_class":"host","phase":"Request","message":"exec request started","request_id":"req-1","context":{"vm_id":"vm-a"}}
{"schema_version":2,"timestamp_unix_ms":3000,"event_kind":"phase_completed","source_class":"host","phase":"Stop","message":"stop complete","request_id":"req-2"}
"#,
        )
        .unwrap();
        std::fs::write(
            run_dir.join(CONSOLE_LOG),
            "[1970-01-01T00:00:01Z] [Exec] [req-1] INFO guest line\nM80_GUEST_BOOT name=ready elapsed_us=7 delta_us=7\n",
        )
        .unwrap();
    }

    #[test]
    fn logs_json_interleaves_host_and_guest_by_timestamp() {
        let temp = tempfile::tempdir().unwrap();
        write_logs(temp.path());
        let filter = parse_filter(None, None).unwrap();
        let mut cursor = FollowCursor::default();

        let output = logs_output(temp.path(), "vm-a", filter, &mut cursor).unwrap();
        let rendered = json::to_pretty(&output);
        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();

        assert_eq!(parsed["data"]["version"], 1);
        assert_eq!(parsed["data"]["records"][0]["source"], "guest");
        assert_eq!(parsed["data"]["records"][1]["source"], "host");
        assert_eq!(parsed["data"]["records"][2]["request_id"], "req-2");
    }

    #[test]
    fn request_id_filter_has_zero_false_positives() {
        let temp = tempfile::tempdir().unwrap();
        write_logs(temp.path());
        let filter = parse_filter(Some("req-1"), None).unwrap();
        let mut cursor = FollowCursor::default();

        let output = logs_output(temp.path(), "vm-a", filter, &mut cursor).unwrap();

        assert_eq!(output.records.len(), 2);
        assert!(
            output
                .records
                .iter()
                .all(|record| record.request_id.as_deref() == Some("req-1")),
            "filtered records must all match the requested id"
        );
    }

    #[test]
    fn follow_cursor_emits_only_newly_appended_records() {
        let temp = tempfile::tempdir().unwrap();
        write_logs(temp.path());
        let filter = parse_filter(None, None).unwrap();
        let mut cursor = FollowCursor::default();

        let first = logs_output(temp.path(), "vm-a", filter, &mut cursor).unwrap();
        assert_eq!(first.records.len(), 4);

        std::fs::OpenOptions::new()
            .append(true)
            .open(temp.path().join(DIAGNOSTICS_FILE_NAME))
            .and_then(|mut file| {
                use std::io::Write as _;
                writeln!(
                    file,
                    r#"{{"schema_version":2,"timestamp_unix_ms":4000,"event_kind":"lifecycle","source_class":"host","phase":"Request","message":"second poll","request_id":"req-3"}}"#
                )
            })
            .unwrap();

        let second = logs_output(temp.path(), "vm-a", filter, &mut cursor).unwrap();
        assert_eq!(second.records.len(), 1);
        assert_eq!(second.records[0].request_id.as_deref(), Some("req-3"));
    }

    #[test]
    fn since_filter_accepts_rfc3339_or_unix_ms() {
        assert_eq!(
            parse_since("1970-01-01T00:00:01Z").unwrap(),
            parse_since("1000").unwrap()
        );
    }
}
