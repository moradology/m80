use std::io::Write;
use std::os::unix::net::UnixListener;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use m80_proto::READY_PORT_DEFAULT;

use super::ready::accept_ready_signal;
use super::*;
use crate::WireProtocolError;

#[test]
fn ready_listener_path_uses_muxer_port_suffix() {
    let vsock = Path::new("/run/m80/vm/firecracker/vm/root/vsock.sock");

    assert_eq!(
        ready_listener_path(vsock),
        PathBuf::from(format!(
            "/run/m80/vm/firecracker/vm/root/vsock.sock_{}",
            READY_PORT_DEFAULT
        ))
    );
}

#[test]
fn ready_signal_accepts_protocol_version_byte() {
    let dir = tempfile::tempdir().unwrap();
    let ready_path = dir.path().join("ready.sock");
    let listener = UnixListener::bind(&ready_path).unwrap();
    let client_path = ready_path.clone();
    let client = std::thread::spawn(move || {
        let mut stream = UnixStream::connect(client_path).unwrap();
        stream
            .write_all(&[m80_proto::PROTOCOL_VERSION as u8])
            .unwrap();
    });

    accept_ready_signal(&listener, &ready_path, Duration::from_secs(1)).unwrap();

    client.join().unwrap();
}

#[test]
fn ready_timeout_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let ready_path = dir.path().join("ready.sock");
    let listener = UnixListener::bind(&ready_path).unwrap();

    let err = accept_ready_signal(&listener, &ready_path, Duration::from_millis(1)).unwrap_err();

    assert!(
        matches!(err, FcError::GuestdReadyTimeout { ref path, timeout }
            if *path == ready_path && timeout == Duration::from_millis(1)),
        "unexpected error: {err:?}"
    );
}

#[test]
fn ready_signal_rejects_wrong_protocol_version() {
    let dir = tempfile::tempdir().unwrap();
    let ready_path = dir.path().join("ready.sock");
    let listener = UnixListener::bind(&ready_path).unwrap();
    let client_path = ready_path.clone();
    let client = std::thread::spawn(move || {
        let mut stream = UnixStream::connect(client_path).unwrap();
        stream.write_all(&[0]).unwrap();
    });

    let err = accept_ready_signal(&listener, &ready_path, Duration::from_secs(1)).unwrap_err();

    assert!(
        matches!(
            err,
            FcError::Protocol(WireProtocolError::UnsupportedVersion {
                expected: m80_proto::PROTOCOL_VERSION,
                got: 0
            })
        ),
        "unexpected error: {err:?}"
    );
    client.join().unwrap();
}

#[test]
fn guest_vsock_port_is_9001() {
    assert_eq!(GUEST_PORT_DEFAULT, 9001);
}
