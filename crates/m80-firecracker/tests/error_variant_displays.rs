//! Each `FcError` variant renders via `Display` without panic.

use m80_firecracker::FcError;

#[test]
fn admission_refused_displays() {
    let e = FcError::AdmissionRefused { limit: 8 };
    let s = e.to_string();
    assert!(s.contains("8"), "expected limit in message, got: {s}");
}

#[test]
fn invalid_state_displays() {
    let e = FcError::InvalidState { expected: "Running", actual: "Stopped" };
    let s = e.to_string();
    assert!(s.contains("Running"), "expected 'Running' in message, got: {s}");
    assert!(s.contains("Stopped"), "expected 'Stopped' in message, got: {s}");
}

#[test]
fn io_error_displays() {
    let e = FcError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"));
    let s = e.to_string();
    assert!(s.contains("no such file"), "expected source message in display, got: {s}");
}

#[test]
fn config_error_displays() {
    let e = FcError::Config("test config error".into());
    let s = e.to_string();
    assert!(s.contains("test config error"), "got: {s}");
}
