//! Each `FcError` variant renders via `Display` without panic.

use m80_firecracker::{ConfigError, FcError};

#[test]
fn admission_refused_displays() {
    let e = FcError::AdmissionRefused { limit: 8 };
    let s = e.to_string();
    assert!(s.contains("8"), "expected limit in message, got: {s}");
}

#[test]
fn invalid_state_displays() {
    let e = FcError::InvalidState {
        expected: "Running",
        actual: "Stopped",
    };
    let s = e.to_string();
    assert!(
        s.contains("Running"),
        "expected 'Running' in message, got: {s}"
    );
    assert!(
        s.contains("Stopped"),
        "expected 'Stopped' in message, got: {s}"
    );
}

#[test]
fn api_socket_timeout_is_typed() {
    let e = FcError::ApiSocketTimeout {
        path: "/run/m80/vm/firecracker.sock".into(),
        timeout: std::time::Duration::from_secs(5),
    };
    let s = e.to_string();
    assert!(s.contains("firecracker.sock"), "got: {s}");
    assert!(s.contains("5s"), "got: {s}");
}

#[test]
fn guestd_ready_timeout_is_typed() {
    let e = FcError::GuestdReadyTimeout {
        path: "/run/m80/vm/vsock.sock_9000".into(),
        timeout: std::time::Duration::from_secs(60),
    };
    let s = e.to_string();
    assert!(s.contains("vsock.sock_9000"), "got: {s}");
    assert!(s.contains("60s"), "got: {s}");
}

#[test]
fn one_shot_consumed_displays() {
    let e = FcError::OneShotConsumed;
    let s = e.to_string();
    assert!(s.contains("one-shot"), "got: {s}");
}

#[test]
fn lifecycle_failure_kinds_are_bounded() {
    use m80_firecracker::LifecycleFailureKind;

    assert_eq!(LifecycleFailureKind::ALL.len(), 7);
    assert_eq!(
        LifecycleFailureKind::ALL,
        [
            LifecycleFailureKind::GuestdNotReady,
            LifecycleFailureKind::BrokenVsock,
            LifecycleFailureKind::StuckVm,
            LifecycleFailureKind::GracefulStopTimeout,
            LifecycleFailureKind::ForcedKillFallback,
            LifecycleFailureKind::CleanupFailure,
            LifecycleFailureKind::WritebackSkippedAfterUncleanStop,
        ]
    );
}

#[test]
fn io_error_displays() {
    let e = FcError::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "no such file",
    ));
    let s = e.to_string();
    assert!(
        s.contains("no such file"),
        "expected source message in display, got: {s}"
    );
}

#[test]
fn config_error_displays() {
    let e = FcError::Config(ConfigError::InvalidValue {
        field: "test",
        reason: "test config error".into(),
    });
    let s = e.to_string();
    assert!(s.contains("test config error"), "got: {s}");
}
