//! Minimal fixture HTTP server over a Unix domain socket.
//!
//! Binds a [`UnixListener`] in a temp dir, accepts one connection per response,
//! serves caller-supplied fixture responses in order, and records every raw
//! request for assertion.
//!
//! Two server types cover the common cases:
//! - [`SingleFixtureServer`] — exactly one request/response; `.join()` returns
//!   a [`FixtureResult`] with `.request: String`.
//! - [`MultiFixtureServer`] — N responses in sequence; `.join()` returns
//!   `Vec<String>`.
//!
//! Crate-specific convenience wrappers (`setup_with_204`, `resp_204`, etc.)
//! live in the consumer crate's own `fixture_server` module.

#![allow(dead_code)]

use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::thread::{self, JoinHandle};

use tempfile::TempDir;

// ── Single-response server ───────────────────────────────────────────────────

/// A running fixture server for exactly one request/response exchange.
///
/// `.join()` returns a [`FixtureResult`].
pub struct SingleFixtureServer {
    pub socket_path: std::path::PathBuf,
    pub _dir: TempDir,
    handle: JoinHandle<FixtureResult>,
}

impl SingleFixtureServer {
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
        Ok(Self {
            socket_path,
            _dir: dir,
            handle,
        })
    }

    /// Wait for the server thread and return what it captured.
    pub fn join(self) -> FixtureResult {
        self.handle.join().expect("fixture server panicked")
    }
}

/// What the single-exchange fixture server captured + served.
pub struct FixtureResult {
    /// The raw text of the HTTP request received (headers + body).
    pub request: String,
}

// ── Multi-response server ────────────────────────────────────────────────────

/// A running fixture server that serves N responses in sequence, one per
/// inbound connection.
///
/// `.join()` returns `Vec<String>` — one entry per exchange.
pub struct MultiFixtureServer {
    /// Path the listener is bound to.
    pub socket_path: std::path::PathBuf,
    /// Keep alive until after the test.
    pub _dir: TempDir,
    handle: JoinHandle<Vec<String>>,
}

impl MultiFixtureServer {
    /// Spawn a server that serves `responses[i]` to the i-th inbound
    /// connection and captures each raw request.
    ///
    /// The client must reconnect between calls — one `UnixStream` per
    /// request, matching how `m80-firecracker-client::Client::new` works.
    pub fn spawn(responses: Vec<Vec<u8>>) -> io::Result<Self> {
        let dir = tempfile::tempdir()?;
        let socket_path = dir.path().join("fc.sock");
        let listener = UnixListener::bind(&socket_path)?;
        let handle = thread::spawn(move || {
            let mut captured = Vec::new();
            for response in responses {
                let (mut conn, _) = listener.accept().expect("accept");
                let req = read_full_request(&mut conn);
                conn.write_all(&response).expect("write response");
                captured.push(req);
            }
            captured
        });
        Ok(Self {
            socket_path,
            _dir: dir,
            handle,
        })
    }

    /// Wait for the server thread and return all captured requests in order.
    pub fn join(self) -> Vec<String> {
        self.handle.join().expect("fixture server panicked")
    }
}

// ── HTTP request reader ──────────────────────────────────────────────────────

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
            let total = hdr_len + content_length;
            expected_total = Some(total);
            if buf.len() >= total {
                break;
            }
        }
    }

    String::from_utf8(buf).expect("request was not valid UTF-8")
}

// ── Common response builders ─────────────────────────────────────────────────

/// Build a `204 No Content` response.
pub fn resp_204() -> Vec<u8> {
    b"HTTP/1.1 204 No Content\r\n\r\n".to_vec()
}

/// Build a `400 Bad Request` response with a JSON body.
pub fn resp_400(body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )
    .into_bytes()
}
