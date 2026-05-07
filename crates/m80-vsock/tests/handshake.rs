mod common;

use common::{HandshakeBehavior, spawn_fake_firecracker_uds};
use m80_vsock::{Channel, VsockError, GUEST_PORT_DEFAULT};
use tempfile::tempdir;

#[test]
fn successful_handshake_opens_channel() {
    let (_dir, path, server) =
        spawn_fake_firecracker_uds(HandshakeBehavior::OkThenHold { port: 12345, hold_ms: 50 });

    let channel = Channel::open_uds_only(&path, GUEST_PORT_DEFAULT).unwrap();
    channel.close().unwrap();

    server.join().unwrap();
}

#[test]
fn bad_handshake_reply_returns_handshake_failed() {
    let (_dir, path, server) = spawn_fake_firecracker_uds(HandshakeBehavior::BadReply);

    let err = Channel::open_uds_only(&path, GUEST_PORT_DEFAULT).unwrap_err();

    assert!(
        matches!(err, VsockError::HandshakeFailed),
        "expected HandshakeFailed, got {err:?}"
    );

    server.join().unwrap();
}

#[test]
fn connect_to_nonexistent_uds_returns_connect_failed() {
    let dir = tempdir().unwrap();
    let uds_path = dir.path().join("does_not_exist.sock");

    let err = Channel::open_uds_only(&uds_path, GUEST_PORT_DEFAULT).unwrap_err();

    assert!(
        matches!(err, VsockError::ConnectFailed { .. }),
        "expected ConnectFailed, got {err:?}"
    );
}
