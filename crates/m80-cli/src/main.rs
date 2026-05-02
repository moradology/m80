//! `m80` — host-side CLI. Thin shell over `m80-firecracker`.
//!
//! See `README.md` for the contract.
//! Behavior captures: dossier `09-cli-shape.md` plus all the L1 epics
//! the CLI exposes as subcommands.
//!
//! # Type-pinning pass
//!
//! Subcommand surface declared; bodies are `todo!()`.

#![deny(missing_docs)]

use std::path::PathBuf;

use m80_firecracker::NetworkPolicy;

/// Top-level argv parse target.
#[derive(Debug)]
pub struct Args {
    /// Emit `--json` machine-readable output where applicable.
    pub json: bool,
    /// Selected subcommand.
    pub subcommand: Subcommand,
}

/// One CLI subcommand.
#[derive(Debug)]
pub enum Subcommand {
    /// Run the host capability checklist; render a table.
    Preflight,
    /// Boot a VM in the foreground.
    Launch {
        /// Optional host workspace to hydrate into a scratch ext4.
        workspace: Option<PathBuf>,
        /// Network policy.
        network: NetworkPolicy,
        /// Optional caller-supplied VM id.
        id: Option<String>,
    },
    /// Send one exec request to a running VM.
    Exec {
        /// VM id.
        vm_id: String,
        /// argv to run inside the VM.
        argv: Vec<String>,
        /// Optional cwd inside the VM.
        cwd: Option<String>,
        /// Optional env overrides (`KEY=VAL`).
        env: Vec<String>,
        /// Optional timeout (milliseconds).
        timeout_ms: Option<u64>,
    },
    /// Stop a running VM, optionally extracting changes.
    Stop {
        /// VM id.
        vm_id: String,
        /// Optional destination for change-extract.
        extract_changes: Option<PathBuf>,
    },
    /// Inspect a VM's run-dir and recorded state.
    Inspect {
        /// VM id.
        vm_id: String,
    },
    /// List run-roots and per-VM ownership.
    List,
    /// Run recovery scans (run-root + orphan bridge).
    Cleanup {
        /// Force cleanup even when state is ambiguous.
        force: bool,
    },
    /// Print the merged effective config with each field's source.
    ConfigShow,
    /// Print the binary version + protocol version + Firecracker pin.
    Version,
}

fn parse_args() -> anyhow::Result<Args> {
    todo!()
}

fn run(_args: Args) -> anyhow::Result<i32> {
    todo!()
}

fn main() -> anyhow::Result<()> {
    let args = parse_args()?;
    let exit_code = run(args)?;
    std::process::exit(exit_code);
}
