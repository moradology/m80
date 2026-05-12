//! Shared test fixtures and utilities for m80 integration tests.
//!
//! This crate is a `[dev-dependencies]` only — it must never appear in a
//! `[dependencies]` block. Its only consumers are test harnesses in other
//! m80 crates.

#![allow(missing_docs)]

pub mod env;
pub mod fixture_server;
