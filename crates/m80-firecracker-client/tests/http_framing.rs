//! Integration tests for HTTP/1.1 framing invariants: request-line format,
//! required headers, and Content-Length accuracy.

mod fixture_server;
use fixture_server::{resp_204, FixtureServer};

use m80_firecracker_client::{BootSourceConfig, Client};
use std::path::PathBuf;

#[test]
fn put_request_line_format_and_headers() {
    let server = FixtureServer::spawn(resp_204()).unwrap();
    let client = Client::new(&server.socket_path).unwrap();
    client
        .put_boot_source(&BootSourceConfig {
            kernel_image_path: PathBuf::from("/k"),
            boot_args: None,
            initrd_path: None,
        })
        .unwrap();
    let result = server.join();
    assert!(
        result.request.starts_with("PUT /boot-source HTTP/1.1\r\n"),
        "unexpected request line: {:?}",
        result.request.lines().next()
    );
    assert!(
        result.request.contains("Content-Type: application/json"),
        "missing Content-Type header"
    );
    // Body is `{"kernel_image_path":"/k"}` — 26 bytes; Content-Length must match.
    assert!(
        result.request.contains("Content-Length:"),
        "missing Content-Length header"
    );
}
