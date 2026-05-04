//! Verify that dropping a `Channel` removes the host-side UDS file.


use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::time::Duration;

use tempfile::tempdir;

use m80_vsock::{Channel, GUEST_PORT_DEFAULT};


#[test]
fn drop_removes_host_uds() {
    let dir = tempdir().unwrap();
    let uds_path = dir.path().join("vsock.sock");

    let listener = UnixListener::bind(&uds_path).unwrap();

    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        reader.get_mut().write_all(b"OK 33333\n").unwrap();
        // Hold the connection open until the client drops.
        std::thread::sleep(Duration::from_millis(200));
    });

    let channel = Channel::open_uds_only(&uds_path, GUEST_PORT_DEFAULT)
    .unwrap();

    // UDS must still exist while the channel is open.
    assert!(uds_path.exists(), "UDS should exist while channel is open");

    drop(channel);

    // After drop the UDS must be removed.
    assert!(
        !uds_path.exists(),
        "UDS should be removed after Channel is dropped"
    );

    server.join().unwrap();
}

#[test]
fn close_removes_host_uds() {
    let dir = tempdir().unwrap();
    let uds_path = dir.path().join("vsock2.sock");

    let listener = UnixListener::bind(&uds_path).unwrap();

    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        reader.get_mut().write_all(b"OK 44444\n").unwrap();
        std::thread::sleep(Duration::from_millis(200));
    });

    let channel = Channel::open_uds_only(&uds_path, GUEST_PORT_DEFAULT)
    .unwrap();

    channel.close().unwrap();

    assert!(
        !uds_path.exists(),
        "UDS should be removed after Channel::close()"
    );

    server.join().unwrap();
}
