//! Structured guest stderr format tests.

use m80_guestd::guest_log::{format_line, GuestLogLevel, GuestLogPhase};

fn parse_line(line: &str) -> (&str, &str, &str, &str, &str) {
    let rest = line.strip_prefix('[').expect("timestamp starts");
    let (timestamp, rest) = rest.split_once("] [").expect("timestamp closes");
    let (phase, rest) = rest.split_once("] [").expect("phase closes");
    let (request_id, rest) = rest.split_once("] ").expect("request id closes");
    let (level, message) = rest.split_once(' ').expect("level/message split");
    (timestamp, phase, request_id, level, message)
}

#[test]
fn one_hundred_guest_log_lines_match_documented_shape() {
    for i in 0..100 {
        let request_id = format!("req-{i}");
        let message = format!("line {i}");
        let line = format_line(
            "2026-05-05T12:34:56Z",
            GuestLogPhase::Exec,
            Some(&request_id),
            GuestLogLevel::Info,
            &message,
        );
        let (timestamp, phase, parsed_request_id, level, parsed_message) = parse_line(&line);
        assert_eq!(timestamp, "2026-05-05T12:34:56Z");
        assert_eq!(phase, "Exec");
        assert_eq!(parsed_request_id, request_id);
        assert_eq!(level, "INFO");
        assert_eq!(parsed_message, message);
    }
}

#[test]
fn missing_request_id_formats_as_boot() {
    let line = format_line(
        "2026-05-05T12:34:56Z",
        GuestLogPhase::Boot,
        None,
        GuestLogLevel::Warn,
        "mount delayed",
    );
    let (_, phase, request_id, level, message) = parse_line(&line);
    assert_eq!(phase, "Boot");
    assert_eq!(request_id, "boot");
    assert_eq!(level, "WARN");
    assert_eq!(message, "mount delayed");
}
