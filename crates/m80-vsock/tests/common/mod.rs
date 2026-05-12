//! Shared test helpers for m80-vsock integration tests.

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::Duration;

use tempfile::TempDir;

/// What the fake Firecracker UDS server does after accepting a connection.
pub(crate) enum HandshakeBehavior {
    /// Complete the handshake (`OK N\n`) then hold the connection briefly.
    OkThenHold { port: u32, hold_ms: u64 },
    /// Complete the handshake with a bad reply (triggers `HandshakeFailed`).
    BadReply,
}

/// Spawn a fake Firecracker UDS server that performs a single handshake.
///
/// Returns `(dir, path, join_handle)` where `dir` keeps the temp directory
/// alive, `path` is the UDS socket path, and `join_handle` lets the caller
/// wait for the server thread to finish.
///
/// The server closes the connection (and exits the thread) after completing
/// the configured [`HandshakeBehavior`].
pub(crate) fn spawn_fake_firecracker_uds(
    behavior: HandshakeBehavior,
) -> (TempDir, PathBuf, JoinHandle<()>) {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("vsock.sock");
    let listener = UnixListener::bind(&path).expect("bind UDS");

    let handle = std::thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept");
        let mut reader = BufReader::new(stream);

        let mut line = String::new();
        reader.read_line(&mut line).expect("read CONNECT");

        match behavior {
            HandshakeBehavior::OkThenHold { port, hold_ms } => {
                let reply = format!("OK {port}\n");
                reader
                    .get_mut()
                    .write_all(reply.as_bytes())
                    .expect("write OK");
                std::thread::sleep(Duration::from_millis(hold_ms));
            }
            HandshakeBehavior::BadReply => {
                reader
                    .get_mut()
                    .write_all(b"ERROR nope\n")
                    .expect("write ERROR");
            }
        }
    });

    (dir, path, handle)
}
