//! Fixture server wrappers for `m80-firecracker-client` integration tests.
//!
//! The generic scaffolding lives in `m80-test-helpers`. This module re-exports
//! the single-response server as `FixtureServer` (the name the existing tests
//! use) and adds `setup_with_204` which is specific to this crate.

#![allow(dead_code, unused_imports)]

pub use m80_test_helpers::fixture_server::{resp_204, resp_400, FixtureResult};
// Alias: existing tests call `FixtureServer::spawn(bytes)` → single-response.
pub use m80_test_helpers::fixture_server::SingleFixtureServer as FixtureServer;

/// Spawn a fixture server that returns 204 and open a client connected to it.
///
/// Call `server.join()` after the client call completes to capture the request.
pub fn setup_with_204() -> (FixtureServer, m80_firecracker_client::Client) {
    let server = FixtureServer::spawn(resp_204()).unwrap();
    let client = m80_firecracker_client::Client::new(&server.socket_path).unwrap();
    (server, client)
}
