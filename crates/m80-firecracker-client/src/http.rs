//! Minimal HTTP/1.1-over-`UnixStream` framing helpers.
//!
//! Only the subset needed to speak the Firecracker control-plane API:
//! - PUT requests with a JSON body
//! - Response parsing: status line + Content-Length-bounded body
//! - No chunked transfer, no connection keep-alive management

use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;

/// A parsed HTTP response (status code + raw body bytes).
#[derive(Debug)]
pub(crate) struct Response {
    pub(crate) status: u16,
    pub(crate) body: Vec<u8>,
}

/// Write a PUT request with a JSON body and read back the response.
pub(crate) fn put_json(stream: &mut UnixStream, path: &str, body: &[u8]) -> io::Result<Response> {
    let header = format!(
        "PUT {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;
    read_response(stream)
}

/// Read a full HTTP/1.1 response from `stream`.
///
/// Firecracker does not use chunked transfer encoding; all responses carry
/// either a `Content-Length` header or no body (1xx / 204 / 304). We read
/// until we have the declared number of body bytes and then stop.
fn read_response(stream: &mut UnixStream) -> io::Result<Response> {
    let mut buf: Vec<u8> = Vec::with_capacity(512);
    let mut chunk = [0u8; 4096];

    // Read until we can determine the expected total length and have it all.
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => {
                // EOF — acceptable only if we can determine completion from
                // what we have.
                match response_end(&buf) {
                    Some(end) => {
                        buf.truncate(end);
                        break;
                    }
                    None => {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "connection closed before full HTTP response was received",
                        ))
                    }
                }
            }
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if let Some(end) = response_end(&buf) {
                    buf.truncate(end);
                    break;
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::ConnectionReset | io::ErrorKind::WouldBlock
                ) =>
            {
                match response_end(&buf) {
                    Some(end) => {
                        buf.truncate(end);
                        break;
                    }
                    None => return Err(e),
                }
            }
            Err(e) => return Err(e),
        }
    }

    parse_response(&buf)
}

/// Returns `Some(total_bytes)` when `buf` contains a complete HTTP response,
/// `None` when more data is needed. `None` is also the answer for malformed
/// headers (truncated UTF-8, no status line) — the caller will keep reading
/// and `parse_response` surfaces the error once a full response arrives.
fn response_end(buf: &[u8]) -> Option<usize> {
    let header_end = buf.windows(4).position(|w| w == b"\r\n\r\n")?;
    let header_len = header_end + 4;
    let header_text = std::str::from_utf8(&buf[..header_end]).ok()?;
    let status = status_from_header_text(header_text)?;
    if matches!(status, 100..=199 | 204 | 304) {
        return Some(header_len);
    }
    let content_length = content_length_from_header_text(header_text)?;
    let total = header_len + content_length;
    if buf.len() >= total { Some(total) } else { None }
}

/// Parse a complete HTTP response from `bytes`.
fn parse_response(bytes: &[u8]) -> io::Result<Response> {
    let header_end = bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "missing HTTP header terminator")
        })?;

    let header_text = std::str::from_utf8(&bytes[..header_end]).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, "HTTP headers are not valid UTF-8")
    })?;

    let status = status_from_header_text(header_text).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "malformed HTTP status line")
    })?;

    let body = bytes[(header_end + 4)..].to_vec();
    Ok(Response { status, body })
}

fn status_from_header_text(header_text: &str) -> Option<u16> {
    let status_line = header_text.lines().next()?.trim_end_matches('\r');
    let mut parts = status_line.splitn(3, ' ');
    let proto = parts.next()?;
    if !proto.starts_with("HTTP/1.") {
        return None;
    }
    parts.next()?.parse::<u16>().ok()
}

fn content_length_from_header_text(header_text: &str) -> Option<usize> {
    header_text.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case("content-length") {
            value.trim().parse::<usize>().ok()
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_end_returns_none_for_incomplete_headers() {
        assert!(response_end(b"HTTP/1.1 200 OK\r\n").is_none());
    }

    #[test]
    fn response_end_returns_header_len_for_204() {
        let resp = b"HTTP/1.1 204 No Content\r\nServer: Firecracker\r\n\r\n";
        assert_eq!(response_end(resp), Some(resp.len()));
    }

    #[test]
    fn response_end_returns_none_until_body_complete() {
        // Content-Length: 5 but only 3 body bytes present.
        let resp = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhel";
        assert!(response_end(resp).is_none());
    }

    #[test]
    fn response_end_returns_total_when_body_complete() {
        let resp = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";
        assert_eq!(response_end(resp), Some(resp.len()));
    }

    #[test]
    fn parse_response_extracts_status_and_body() {
        let resp = b"HTTP/1.1 400 Bad Request\r\nContent-Length: 7\r\n\r\nbad req";
        let r = parse_response(resp).unwrap();
        assert_eq!(r.status, 400);
        assert_eq!(r.body, b"bad req");
    }

    #[test]
    fn parse_response_empty_body_for_204() {
        let resp = b"HTTP/1.1 204 No Content\r\n\r\n";
        let r = parse_response(resp).unwrap();
        assert_eq!(r.status, 204);
        assert!(r.body.is_empty());
    }

    #[test]
    fn parse_response_error_on_missing_header_terminator() {
        let err = parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn put_request_line_format() {
        use std::io::BufRead;
        use std::os::unix::net::UnixListener;
        use std::thread;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let sock = dir.path().join("t.sock");
        let listener = UnixListener::bind(&sock).unwrap();

        let server = thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            // Read until \r\n\r\n to capture the request line.
            let mut req = String::new();
            let mut reader = io::BufReader::new(&mut conn);
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                req.push_str(&line);
            }
            // Drain body.
            let content_length: usize = req
                .lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    if k.trim().eq_ignore_ascii_case("content-length") {
                        v.trim().parse().ok()
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            let mut body = vec![0u8; content_length];
            io::Read::read_exact(&mut reader, &mut body).unwrap();
            drop(reader);
            // Send fixture response.
            io::Write::write_all(
                &mut conn,
                b"HTTP/1.1 204 No Content\r\n\r\n",
            )
            .unwrap();
            req
        });

        let mut stream = UnixStream::connect(&sock).unwrap();
        put_json(&mut stream, "/boot-source", br#"{"k":"v"}"#).unwrap();
        let req = server.join().unwrap();
        assert!(
            req.starts_with("PUT /boot-source HTTP/1.1\r\n"),
            "unexpected request line: {req:?}"
        );
        assert!(req.contains("Content-Type: application/json"));
        assert!(req.contains("Content-Length: 9"));
    }
}
