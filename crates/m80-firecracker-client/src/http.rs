//! Minimal HTTP/1.1-over-`UnixStream` framing helpers.
//!
//! Only the subset needed to speak the Firecracker control-plane API:
//! - PUT requests with a JSON body
//! - Response parsing: status line + Content-Length-bounded body
//! - No chunked transfer, no connection keep-alive management

use std::io::{self, Read, Write};

/// A parsed HTTP response (status code + raw body bytes).
#[derive(Debug)]
pub(crate) struct Response {
    pub(crate) status: u16,
    pub(crate) body: Vec<u8>,
}

pub(crate) fn send_json(
    stream: &mut (impl Read + Write),
    method: &str,
    path: &str,
    body: &[u8],
) -> io::Result<Response> {
    let header = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    let mut request = Vec::with_capacity(header.len() + body.len());
    request.extend_from_slice(header.as_bytes());
    request.extend_from_slice(body);
    stream.write_all(&request)?;
    stream.flush()?;
    read_response(stream)
}

/// Read a full HTTP/1.1 response from `stream`.
///
/// Firecracker does not use chunked transfer encoding; all responses carry
/// either a `Content-Length` header or no body (1xx / 204 / 304). We read
/// until we have the declared number of body bytes and then stop.
fn read_response(stream: &mut impl Read) -> io::Result<Response> {
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
    let status = {
        let line = header_text.lines().next()?.trim_end_matches('\r');
        let mut p = line.splitn(3, ' ');
        if !p.next()?.starts_with("HTTP/1.") {
            return None;
        }
        p.next()?.parse::<u16>().ok()?
    };
    if matches!(status, 100..=199 | 204 | 304) {
        return Some(header_len);
    }
    let content_length = header_text.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case("content-length") {
            value.trim().parse::<usize>().ok()
        } else {
            None
        }
    })?;
    let total = header_len + content_length;
    if buf.len() >= total {
        Some(total)
    } else {
        None
    }
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
        io::Error::new(
            io::ErrorKind::InvalidData,
            "HTTP headers are not valid UTF-8",
        )
    })?;

    let status = {
        let line = header_text.lines().next().and_then(|l| {
            let l = l.trim_end_matches('\r');
            let mut p = l.splitn(3, ' ');
            if !p.next()?.starts_with("HTTP/1.") {
                return None;
            }
            p.next()?.parse::<u16>().ok()
        });
        line.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "malformed HTTP status line")
        })?
    };

    let body = bytes[(header_end + 4)..].to_vec();
    Ok(Response { status, body })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

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
    fn send_json_writes_header_and_body_in_one_call() {
        let response = b"HTTP/1.1 204 No Content\r\n\r\n".to_vec();
        let mut stream = CountingStream {
            response: Cursor::new(response),
            writes: Vec::new(),
            flushes: 0,
        };

        let result = send_json(
            &mut stream,
            "PUT",
            "/actions",
            br#"{"action_type":"InstanceStart"}"#,
        )
        .unwrap();

        assert_eq!(result.status, 204);
        assert_eq!(stream.writes.len(), 1);
        assert_eq!(stream.flushes, 1);
        let request = std::str::from_utf8(&stream.writes[0]).unwrap();
        assert!(request.starts_with("PUT /actions HTTP/1.1\r\n"));
        assert!(request.ends_with("\r\n\r\n{\"action_type\":\"InstanceStart\"}"));
    }

    struct CountingStream {
        response: Cursor<Vec<u8>>,
        writes: Vec<Vec<u8>>,
        flushes: usize,
    }

    impl Read for CountingStream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.response.read(buf)
        }
    }

    impl Write for CountingStream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.writes.push(buf.to_vec());
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            Ok(())
        }
    }
}
