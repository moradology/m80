mod common;

use common::{spawn_fake_firecracker_uds, HandshakeBehavior};
use m80_proto::GUEST_PORT_DEFAULT;
use m80_vsock::{Channel, VsockError};
use tempfile::tempdir;

#[test]
fn successful_handshake_opens_channel() {
    let (_dir, path, server) = spawn_fake_firecracker_uds(HandshakeBehavior::OkThenHold {
        port: 12345,
        hold_ms: 50,
    });

    let channel = Channel::open_uds_only(&path, GUEST_PORT_DEFAULT).unwrap();
    drop(channel);

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
fn connect_to_nonexistent_uds_returns_io_error() {
    let dir = tempdir().unwrap();
    let uds_path = dir.path().join("does_not_exist.sock");

    let err = Channel::open_uds_only(&uds_path, GUEST_PORT_DEFAULT).unwrap_err();

    assert!(
        matches!(err, VsockError::Io { .. }),
        "expected Io, got {err:?}"
    );
}
