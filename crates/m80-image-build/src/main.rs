//! `m80-image-build` — build the m80 guest image and provenance manifest.
//!
//! See `README.md` for the contract.
//! Behavior captures: bead epic `m80-sz1` (`br show m80-sz1`).
//!
//! # Type-pinning pass
//!
//! Argument structs and subcommand surface declared; bodies are `todo!()`.

#![deny(missing_docs)]

use std::path::PathBuf;

/// Command-line subcommand selection.
#[derive(Debug)]
pub enum Subcommand {
    /// Run the full build pipeline using the config at the given path.
    Run {
        /// Path to the build config TOML.
        config: PathBuf,
        /// Print the plan without performing I/O.
        dry_run: bool,
    },
    /// Re-verify an already-built rootfs against its manifest.
    Verify {
        /// Path to the built rootfs.
        rootfs: PathBuf,
    },
    /// Remove intermediate build artifacts (loop mount points, temp images).
    Clean {
        /// Working directory to clean.
        workdir: PathBuf,
    },
}

/// Top-level argv parse target.
#[derive(Debug)]
pub struct Args {
    /// Selected subcommand.
    pub subcommand: Subcommand,
}

fn parse_args() -> anyhow::Result<Args> {
    todo!()
}

fn run(_args: Args) -> anyhow::Result<()> {
    todo!()
}

fn main() -> anyhow::Result<()> {
    let args = parse_args()?;
    run(args)
}
