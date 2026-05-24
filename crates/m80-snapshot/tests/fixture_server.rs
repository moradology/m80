//! Fixture server wrappers for `m80-snapshot` integration tests.
//!
//! The generic scaffolding lives in `m80-test-helpers`. This module re-exports
//! the multi-response server as `FixtureServer` (the name the existing tests
//! use) and the common response builders.

#![allow(dead_code, unused_imports)]

pub(crate) use m80_test_helpers::fixture_server::{resp_204, resp_400};
// Alias: existing tests call `FixtureServer::spawn(vec![...])` → multi-response.
pub(crate) use m80_test_helpers::fixture_server::MultiFixtureServer as FixtureServer;

pub(crate) fn resp_version(version: &str) -> Vec<u8> {
    let body = format!(r#"{{"firecracker_version":"{version}"}}"#);
    format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )
    .into_bytes()
}
