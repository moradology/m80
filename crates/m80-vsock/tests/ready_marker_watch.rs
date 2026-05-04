use std::io::Write;
use std::time::Duration;

use tempfile::tempdir;

use m80_vsock::{watch_ready_marker, VsockError};

#[test]
fn marker_already_present_returns_immediately() {
    let dir = tempdir().unwrap();
    let console = dir.path().join("console.log");
    std::fs::write(&console, "boot message\nGUESTD_READY\nmore stuff\n").unwrap();

    watch_ready_marker(&console, "GUESTD_READY", Duration::from_secs(1)).unwrap();
}

#[test]
fn marker_appears_after_delay_returns_ok() {
    let dir = tempdir().unwrap();
    let console = dir.path().join("console.log");
    // Start with no marker.
    std::fs::write(&console, "booting...\n").unwrap();

    let console_clone = console.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&console_clone)
            .unwrap();
        writeln!(f, "GUESTD_READY").unwrap();
    });

    watch_ready_marker(&console, "GUESTD_READY", Duration::from_secs(1)).unwrap();
}

#[test]
fn marker_never_appears_returns_not_ready() {
    let dir = tempdir().unwrap();
    let console = dir.path().join("console.log");
    std::fs::write(&console, "still booting...\n").unwrap();

    let err = watch_ready_marker(&console, "GUESTD_READY", Duration::from_millis(150)).unwrap_err();
    assert!(
        matches!(err, VsockError::NotReady),
        "expected NotReady, got {err:?}"
    );
}

#[test]
fn partial_line_containing_marker_does_not_match() {
    let dir = tempdir().unwrap();
    let console = dir.path().join("console.log");
    // "GUESTD_READY_EXTRA" must not trigger a match; the marker must be exact.
    std::fs::write(&console, "GUESTD_READY_EXTRA\n").unwrap();

    let err = watch_ready_marker(&console, "GUESTD_READY", Duration::from_millis(100)).unwrap_err();
    assert!(matches!(err, VsockError::NotReady));
}

#[test]
fn missing_console_file_does_not_panic_before_timeout() {
    let dir = tempdir().unwrap();
    let console = dir.path().join("nonexistent.log");

    // File doesn't exist; should wait and then time out, not panic.
    let err = watch_ready_marker(&console, "GUESTD_READY", Duration::from_millis(100)).unwrap_err();
    assert!(matches!(err, VsockError::NotReady));
}
