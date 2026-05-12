use m80_firecracker::OWNERSHIP_LOCK;
use tempfile::TempDir;

use crate::common;

#[test]
fn ownership_lock_is_the_marker_file() {
    assert_eq!(OWNERSHIP_LOCK, "ownership.lock");
}

#[test]
fn current_process_ownership_lock_preserves_run_dir() {
    let dir = TempDir::new().unwrap();
    let live = dir.path().join("vm-live");
    std::fs::create_dir_all(&live).unwrap();
    std::fs::write(
        live.join(OWNERSHIP_LOCK),
        format!("pid={}\nstarted_at=1\n", std::process::id()),
    )
    .unwrap();

    common::make_fake_backend(8, dir.path()).recover_stale_run_root(false).unwrap();

    assert!(live.exists());
}

#[test]
fn stale_detection_uses_ownership_and_jailer_state_not_socket_probes() {
    let dir = TempDir::new().unwrap();
    let stale = dir.path().join("vm-stale");
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::write(stale.join("firecracker.sock"), b"not a socket").unwrap();
    std::fs::write(stale.join("vsock.sock"), b"not a socket").unwrap();

    common::make_fake_backend(8, dir.path()).recover_stale_run_root(false).unwrap();

    assert!(!stale.exists());
}

#[test]
fn preserves_run_dir_when_ownership_lock_is_ambiguous() {
    let dir = TempDir::new().unwrap();
    let ambiguous = dir.path().join("vm-ambiguous");
    std::fs::create_dir_all(&ambiguous).unwrap();
    std::fs::write(ambiguous.join(OWNERSHIP_LOCK), b"not-parseable").unwrap();

    common::make_fake_backend(8, dir.path()).recover_stale_run_root(false).unwrap();

    assert!(ambiguous.exists());
}
