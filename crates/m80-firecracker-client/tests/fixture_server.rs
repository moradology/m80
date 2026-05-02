//! Helper that binds a `UnixListener` in a temp dir, accepts one connection,
//! serves a caller-supplied fixture response, and returns the raw request.
//!
//! Used by all integration tests to avoid a real Firecracker binary.

#![allow(dead_code)]

use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::thread::{self, JoinHandle};

use tempfile::TempDir;

/// A running fixture server handle.
///
/// The caller should call [`FixtureServer::join`] after the client call
/// completes; panicking inside the server thread surfaces cleanly that way.
pub struct FixtureServer {
    pub socket_path: std::path::PathBuf,
    /// Keep the TempDir alive until after the test.
    pub _dir: TempDir,
    handle: JoinHandle<FixtureResult>,
}

/// What the fixture server captured + sent.
pub struct FixtureResult {
    /// The raw text of the HTTP request received (headers + body).
    pub request: String,
}

impl FixtureServer {
    /// Spawn a fixture server that serves `response_bytes` to the first
    /// inbound connection and captures the request.
    pub fn spawn(response_bytes: Vec<u8>) -> io::Result<Self> {
        let dir = tempfile::tempdir()?;
        let socket_path = dir.path().join("fc.sock");
        let listener = UnixListener::bind(&socket_path)?;
        let handle = thread::spawn(move || {
            let (mut conn, _) = listener.accept().expect("accept");
            let request = read_full_request(&mut conn);
            conn.write_all(&response_bytes).expect("write response");
            FixtureResult { request }
        });
        Ok(Self { socket_path, _dir: dir, handle })
    }

    /// Wait for the server thread and return what it captured.
    pub fn join(self) -> FixtureResult {
        self.handle.join().expect("fixture server panicked")
    }
}

/// Read a complete HTTP request from a `UnixStream` (headers + body bounded
/// by `Content-Length`).
fn read_full_request(stream: &mut UnixStream) -> String {
    let mut buf = Vec::<u8>::new();
    let mut tmp = [0u8; 1024];
    let mut expected_total: Option<usize> = None;

    loop {
        let n = stream.read(&mut tmp).expect("read");
        assert!(n > 0, "unexpected EOF while reading request");
        buf.extend_from_slice(&tmp[..n]);

        if let Some(total) = expected_total {
            if buf.len() >= total {
                break;
            }
            continue;
        }

        if let Some(hdr_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let hdr_len = hdr_end + 4;
            let hdr_text = String::from_utf8_lossy(&buf[..hdr_end]);
            let content_length: usize = hdr_text
                .lines()
                .find_map(|line| {
                    let (k, v) = line.split_once(':')?;
                    if k.trim().eq_ignore_ascii_case("content-length") {
                        v.trim().parse().ok()
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            expected_total = Some(hdr_len + content_length);
            if buf.len() >= hdr_len + content_length {
                break;
            }
        }
    }

    String::from_utf8(buf).expect("request was not valid UTF-8")
}

/// Convenience: build a `204 No Content` response.
pub fn resp_204() -> Vec<u8> {
    b"HTTP/1.1 204 No Content\r\n\r\n".to_vec()
}

/// Convenience: build a `400 Bad Request` with a JSON body.
pub fn resp_400(body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )
    .into_bytes()
}
