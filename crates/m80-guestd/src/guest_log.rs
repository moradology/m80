//! Structured stderr logging for guest-visible diagnostics.

use std::io::Write as _;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Guest-side lifecycle phase for structured stderr lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuestLogPhase {
    /// Boot and init work before guestd is ready.
    Boot,
    /// Ready-signal and vsock listener setup.
    Ready,
    /// Per-request exec handling.
    Exec,
    /// Shutdown request handling.
    Shutdown,
}

impl GuestLogPhase {
    fn as_str(self) -> &'static str {
        match self {
            GuestLogPhase::Boot => "Boot",
            GuestLogPhase::Ready => "Ready",
            GuestLogPhase::Exec => "Exec",
            GuestLogPhase::Shutdown => "Shutdown",
        }
    }
}

/// Guest-side log level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuestLogLevel {
    /// Fatal or operation-failing condition.
    Error,
    /// Recoverable condition worth surfacing.
    Warn,
    /// Normal lifecycle progress.
    Info,
    /// Verbose diagnostics. Emitted only when callers choose to call it.
    #[allow(dead_code)]
    Debug,
}

impl GuestLogLevel {
    fn as_str(self) -> &'static str {
        match self {
            GuestLogLevel::Error => "ERROR",
            GuestLogLevel::Warn => "WARN",
            GuestLogLevel::Info => "INFO",
            GuestLogLevel::Debug => "DEBUG",
        }
    }
}

/// Format one guest log line.
///
/// Shape:
/// `[<RFC3339-timestamp>] [<phase>] [<request_id-or-boot>] <level> <message>`
pub fn format_line(
    timestamp: &str,
    phase: GuestLogPhase,
    request_id: Option<&str>,
    level: GuestLogLevel,
    message: &str,
) -> String {
    let request_id = request_id.unwrap_or("boot");
    format!(
        "[{timestamp}] [{}] [{request_id}] {} {message}",
        phase.as_str(),
        level.as_str()
    )
}

/// Emit one structured line to stderr. Failures are intentionally ignored:
/// logging must not change guest control flow.
pub fn log(
    phase: GuestLogPhase,
    request_id: Option<&str>,
    level: GuestLogLevel,
    message: impl AsRef<str>,
) {
    let timestamp = rfc3339_utc_now();
    let line = format_line(&timestamp, phase, request_id, level, message.as_ref());
    let _ = writeln!(std::io::stderr().lock(), "{line}");
}

fn rfc3339_utc_now() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    rfc3339_from_unix_secs(now.as_secs())
}

fn rfc3339_from_unix_secs(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let second_of_day = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = second_of_day / 3_600;
    let minute = (second_of_day % 3_600) / 60;
    let second = second_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    (year as i32, month as u32, day as u32)
}

/// Emit an `INFO` line.
pub fn info(phase: GuestLogPhase, request_id: Option<&str>, message: impl AsRef<str>) {
    log(phase, request_id, GuestLogLevel::Info, message);
}

/// Emit a `WARN` line.
pub fn warn(phase: GuestLogPhase, request_id: Option<&str>, message: impl AsRef<str>) {
    log(phase, request_id, GuestLogLevel::Warn, message);
}

/// Emit an `ERROR` line.
pub fn error(phase: GuestLogPhase, request_id: Option<&str>, message: impl AsRef<str>) {
    log(phase, request_id, GuestLogLevel::Error, message);
}

/// Monotonic guest boot milestone emitter.
///
/// Lines intentionally use a greppable key=value shape distinct from the
/// human log format so host bench tooling can parse them without depending on
/// message text:
///
/// `M80_GUEST_BOOT name=<name> elapsed_us=<micros> delta_us=<micros>`
#[derive(Debug)]
pub struct BootTimer {
    start: Instant,
    last: Instant,
}

impl BootTimer {
    /// Start a new boot timer at the current monotonic instant.
    pub fn start() -> Self {
        let now = Instant::now();
        Self {
            start: now,
            last: now,
        }
    }

    /// Emit one boot milestone to stderr and advance the delta baseline.
    pub fn mark(&mut self, name: &str) {
        let now = Instant::now();
        let elapsed_us = now.duration_since(self.start).as_micros();
        let delta_us = now.duration_since(self.last).as_micros();
        self.last = now;
        let line = format_boot_milestone_line(name, elapsed_us, delta_us);
        let _ = writeln!(std::io::stderr().lock(), "{line}");
    }
}

/// Format one machine-readable boot milestone line.
pub fn format_boot_milestone_line(name: &str, elapsed_us: u128, delta_us: u128) -> String {
    format!("M80_GUEST_BOOT name={name} elapsed_us={elapsed_us} delta_us={delta_us}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_epoch_formats_as_rfc3339() {
        assert_eq!(rfc3339_from_unix_secs(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn known_leap_day_formats_as_rfc3339() {
        assert_eq!(
            rfc3339_from_unix_secs(1_582_934_400),
            "2020-02-29T00:00:00Z"
        );
    }

    #[test]
    fn boot_milestone_line_is_key_value_parseable() {
        assert_eq!(
            format_boot_milestone_line("overlayfs_mounted", 1234, 56),
            "M80_GUEST_BOOT name=overlayfs_mounted elapsed_us=1234 delta_us=56"
        );
    }
}
