//! `m80-guestd` — in-VM daemon.
//!
//! See `README.md` for the contract.
//! Behavior captures: bead epic `m80-eb8` (`br show m80-eb8`).
//!
//! # Type-pinning pass
//!
//! Argv parse + the wire types (`ExecRequest`/`ExecResponse`) are declared
//! here. **Note**: these mirror `m80_firecracker::Exec*` and must stay
//! JSON-compatible. A future refactor extracts them to a shared crate
//! (`m80-exec`); for now the implementing agent keeps them in sync.

#![deny(missing_docs)]

use serde::{Deserialize, Serialize};

/// Argv parse target.
#[derive(Debug)]
pub struct Args {
    /// Override the default vsock port (testing only).
    pub port: Option<u32>,
    /// Print version and exit.
    pub print_version: bool,
}

/// One exec request, mirroring `m80_firecracker::ExecRequest`. Must stay
/// JSON-compatible — the host serializes its `ExecRequest` and we deserialize
/// it into this struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecRequest {
    /// argv to spawn.
    pub argv: Vec<String>,
    /// Working directory inside the guest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Environment override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<(String, String)>>,
    /// Optional bytes piped to the child's stdin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdin: Option<Vec<u8>>,
    /// Optional bound on running time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

/// One exec response, mirroring `m80_firecracker::ExecResponse`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResponse {
    /// Termination disposition.
    pub status: ExecStatus,
    /// Process exit code.
    pub exit_code: Option<i32>,
    /// Captured stdout.
    pub stdout: Vec<u8>,
    /// Captured stderr.
    pub stderr: Vec<u8>,
    /// Wall-clock timing.
    pub timing: ExecTiming,
}

/// Wall-clock timing, mirroring `m80_firecracker::ExecTiming`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ExecTiming {
    /// Spawn time in unix epoch milliseconds.
    pub spawned_at_unix_ms: u64,
    /// Exit time in unix epoch milliseconds.
    pub exited_at_unix_ms: u64,
    /// Time from `spawn` syscall return to first byte of stdout/stderr.
    pub spawn_ms: u64,
    /// Total wall-clock runtime.
    pub run_ms: u64,
}

/// Termination disposition, mirroring `m80_firecracker::ExecStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecStatus {
    /// Child exited normally.
    Completed,
    /// `timeout_ms` expired.
    TimedOut,
    /// Host disconnected mid-exec.
    Cancelled,
    /// Spawn failed or another guest-side error.
    Failed,
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
