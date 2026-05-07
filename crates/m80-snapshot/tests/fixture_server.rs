//! Fixture server wrappers for `m80-snapshot` integration tests.
//!
//! The generic scaffolding lives in `m80-test-helpers`. This module re-exports
//! the multi-response server as `FixtureServer` (the name the existing tests
//! use) and the common response builders.

#![allow(dead_code)]

pub use m80_test_helpers::fixture_server::{resp_204, resp_400};
// Alias: existing tests call `FixtureServer::spawn(vec![...])` → multi-response.
pub use m80_test_helpers::fixture_server::MultiFixtureServer as FixtureServer;
