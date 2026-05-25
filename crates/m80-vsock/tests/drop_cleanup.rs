mod common;

use common::{spawn_fake_firecracker_uds, HandshakeBehavior};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixListener;

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

#[test]
fn dropping_channel_signals_eof_to_peer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vsock.sock");
    let listener = UnixListener::bind(&path).unwrap();

    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);

        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(line.starts_with("CONNECT "));
        reader.get_mut().write_all(b"OK 33333\n").unwrap();

        let mut buf = [0u8; 1];
        let read = reader.read(&mut buf).unwrap();
        assert_eq!(read, 0, "dropping Channel should close the peer stream");
    });

    let channel = Channel::open_uds_only(&path, GUEST_PORT_DEFAULT).unwrap();
    drop(channel);

    server.join().unwrap();
}
