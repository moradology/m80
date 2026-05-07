//! Minimal fixture HTTP server over a Unix domain socket.
//!
//! Binds a `UnixListener` in a temp dir, accepts one or more connections
//! in sequence, serves a caller-supplied response per connection, and
//! records every raw request for assertion.
//!
//! Used by the capture/restore integration tests to avoid a real Firecracker
//! binary.

use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::thread::{self, JoinHandle};

use tempfile::TempDir;

/// A running fixture server handle that serves N responses in sequence.
#[allow(dead_code)]
pub struct FixtureServer {
    /// Path the listener is bound to.
    pub socket_path: std::path::PathBuf,
    /// Keep alive until after the test.
    pub _dir: TempDir,
    handle: JoinHandle<Vec<String>>,
}

impl FixtureServer {
    /// Spawn a server that serves each `responses[i]` to the i-th inbound
    /// connection and captures each raw request.
    ///
    /// The client must reconnect between calls (one `UnixStream` per
    /// `FirecrackerClient` request, because `Client::new` opens a fresh socket).
    /// Each captured request is pushed to the returned `Vec<String>` in order.
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
