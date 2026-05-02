//! Verify that capture() and restore() return SnapshotError::Deferred in v0.1.

use m80_snapshot::{SnapshotError, capture, restore};

/// `capture()` returns `Deferred` in v0.1.
#[test]
fn capture_returns_deferred() {
    let err = capture().unwrap_err();
    assert!(
        matches!(err, SnapshotError::Deferred),
        "capture must return Deferred in v0.1, got {err:?}"
    );
}

/// `restore()` returns `Deferred` in v0.1.
#[test]
fn restore_returns_deferred() {
    let err = restore().unwrap_err();
    assert!(
        matches!(err, SnapshotError::Deferred),
        "restore must return Deferred in v0.1, got {err:?}"
    );
}
