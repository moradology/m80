mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::time::Duration;

use tempfile::tempdir;

use m80_vsock::{Channel, VsockError, GUEST_PORT_DEFAULT};

use common::console_with_marker;

#[test]
fn successful_handshake_opens_channel() {
    let dir = tempdir().unwrap();
    let uds_path = dir.path().join("vsock.sock");
    let console = console_with_marker(dir.path());

    let listener = UnixListener::bind(&uds_path).unwrap();

    let uds_clone = uds_path.clone();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, format!("CONNECT {GUEST_PORT_DEFAULT}\n"));
        reader.get_mut().write_all(b"OK 12345\n").unwrap();
        // Keep server end open briefly so the client can succeed.
        std::thread::sleep(Duration::from_millis(50));
        drop(uds_clone);
    });

    let channel = Channel::open(
        &uds_path,
        GUEST_PORT_DEFAULT,
        "GUESTD_READY",
        &console,
        Duration::from_secs(1),
    )
    .unwrap();
    channel.close().unwrap();

    server.join().unwrap();
}

#[test]
fn bad_handshake_reply_returns_handshake_failed() {
    let dir = tempdir().unwrap();
    let uds_path = dir.path().join("vsock.sock");
    let console = console_with_marker(dir.path());

    let listener = UnixListener::bind(&uds_path).unwrap();

    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        // Read and discard the CONNECT line.
        let mut buf = [0u8; 64];
        let _ = stream.read(&mut buf);
        stream.write_all(b"ERROR nope\n").unwrap();
    });

    let err = Channel::open(
        &uds_path,
        GUEST_PORT_DEFAULT,
        "GUESTD_READY",
        &console,
        Duration::from_secs(1),
    )
    .unwrap_err();

    assert!(
        matches!(err, VsockError::HandshakeFailed),
        "expected HandshakeFailed, got {err:?}"
    );
}

#[test]
fn connect_to_nonexistent_uds_returns_connect_failed() {
    let dir = tempdir().unwrap();
    let uds_path = dir.path().join("does_not_exist.sock");
    let console = console_with_marker(dir.path());

    let err = Channel::open(
        &uds_path,
        GUEST_PORT_DEFAULT,
        "GUESTD_READY",
        &console,
        Duration::from_secs(1),
    )
    .unwrap_err();

    assert!(
        matches!(err, VsockError::ConnectFailed { .. }),
        "expected ConnectFailed, got {err:?}"
    );
}

// Bring std::io::Read into scope for `read` on raw stream in server thread.
use std::io::Read;
