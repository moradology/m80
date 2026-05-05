//! `m80-cli` library surface.
//!
//! Declares all modules so integration tests (which link the lib target)
//! can use `Cli::try_parse_from` without spawning a subprocess.
//!
//! The `main.rs` binary entry point imports from here.

#![deny(missing_docs)]

pub mod args;
pub mod cmds;
pub mod cmds_walk;
pub mod config;
pub mod errors;

pub use args::{Cli, Cmd, ConfigAction, SnapshotAction};
