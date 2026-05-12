mod common;

use common::{spawn_fake_firecracker_uds, HandshakeBehavior};
use m80_proto::GUEST_PORT_DEFAULT;
use m80_vsock::Channel;

#[test]
fn drop_keeps_host_uds() {
    let (_dir, path, server) = spawn_fake_firecracker_uds(HandshakeBehavior::OkThenHold {
        port: 33333,
        hold_ms: 200,
    });

    let channel = Channel::open_uds_only(&path, GUEST_PORT_DEFAULT).unwrap();

    // UDS must still exist while the channel is open.
    assert!(path.exists(), "UDS should exist while channel is open");

    drop(channel);

    assert!(path.exists(), "UDS should remain after Channel is dropped");

    server.join().unwrap();
}
