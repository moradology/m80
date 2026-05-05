//! Clap-derive argument types for the `m80` CLI.
//!
//! Every subcommand's fields map 1:1 to the fields of the original
//! type-pinning `Subcommand` enum in `main.rs`.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use m80_firecracker::NetworkPolicy;

/// The `m80` command-line tool. Thin shell over `m80-firecracker`.
///
/// Boot Firecracker microVMs, run commands inside them, inspect state,
/// and clean up residue. All operations are local-host; no daemon required.
#[derive(Debug, Parser)]
#[command(name = "m80", about, long_about = None)]
#[command(version)]
pub struct Cli {
    /// Emit machine-readable JSON instead of human-friendly text.
    #[arg(long, global = true)]
    pub json: bool,

    /// Subcommand to run.
    #[command(subcommand)]
    pub subcommand: Cmd,
}

/// All `m80` subcommands.
#[derive(Debug, Subcommand)]
pub enum Cmd {
    /// Run the host capability checklist; render a table.
    ///
    /// Exits 0 on full pass, 2 on any check failed.
    Preflight,

    /// Boot a VM in the foreground. Blocks until interrupted (Ctrl-C).
    ///
    /// Prints the VM id and run-dir on launch. With --exec, runs the
    /// command inside the VM and exits when it completes (single-shot
    /// mode).
    ///
    /// With --from-snapshot, restores from a previously captured snapshot
    /// directory instead of performing a cold boot.
    Launch {
        /// Optional host workspace directory to hydrate into a scratch
        /// ext4 inside the VM.
        #[arg(long, value_name = "PATH")]
        workspace: Option<PathBuf>,

        /// Network policy: noegress (default) or outbound.
        #[arg(long, value_name = "POLICY", default_value = "noegress",
              value_parser = parse_network_policy)]
        network: NetworkPolicy,

        /// Optional caller-supplied VM id (auto-derived when absent).
        #[arg(long, value_name = "VM_ID")]
        id: Option<String>,

        /// Restore from a snapshot directory instead of cold-booting.
        ///
        /// The directory must contain vm.snap and mem.snap (written by
        /// m80 snapshot capture). When set, cold-boot-only flags such as
        /// boot-arg overrides are rejected with an error rather than
        /// silently ignored.
        #[arg(long, value_name = "DIR", conflicts_with = "id")]
        from_snapshot: Option<PathBuf>,

        /// Single-shot exec: run argv inside the VM, then stop.
        /// Pass the full command after '--', e.g.: m80 launch -- /bin/sh -c "echo hi"
        /// When omitted, launch blocks until Ctrl-C.
        #[arg(last = true, value_name = "ARGV")]
        exec: Vec<String>,
    },

    /// Send one exec request to a running VM.
    ///
    /// v0.1 limitation: exec and launch must run in the same process when
    /// using the library. The CLI "exec" subcommand is a v0.2 feature
    /// that requires out-of-process IPC. Use `m80 launch -- <argv>` for
    /// single-shot launch+exec in v0.1.
    Exec {
        /// VM id.
        vm_id: String,

        /// Command and arguments to run inside the VM (after '--').
        #[arg(last = true, value_name = "ARGV")]
        argv: Vec<String>,

        /// Working directory inside the VM.
        #[arg(long, value_name = "PATH")]
        cwd: Option<String>,

        /// Environment overrides in KEY=VAL form (may be repeated).
        #[arg(long = "env", value_name = "KEY=VAL")]
        env: Vec<String>,

        /// Exec timeout in milliseconds.
        #[arg(long, value_name = "MS")]
        timeout_ms: Option<u64>,
    },

    /// Stop a running VM, optionally extracting changes first.
    ///
    /// v0.1 limitation: stop is implemented by walking the run-root and
    /// SIGKILLing recorded pids. Clean stop via IPC is v0.2.
    Stop {
        /// VM id.
        vm_id: String,

        /// Optional destination directory for change extraction.
        #[arg(long, value_name = "DEST")]
        extract_changes: Option<PathBuf>,
    },

    /// Print a VM's run-dir layout and recorded state.
    Inspect {
        /// VM id.
        vm_id: String,
    },

    /// List all VM run-dirs under the run-root.
    List,

    /// Recover stale run-roots and clean up orphan bridges.
    Cleanup {
        /// Force cleanup even when state is ambiguous.
        #[arg(long)]
        force: bool,
    },

    /// Print the merged effective configuration with each field's source.
    #[command(name = "config")]
    Config {
        /// Config sub-action (currently: show).
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Snapshot operations (capture).
    ///
    /// v0.1 limitation: capture requires the VM to be launched in the same
    /// process (out-of-process IPC is v0.2). For library use call
    /// RunningSandbox::capture() directly.
    #[command(name = "snapshot")]
    Snapshot {
        /// Snapshot sub-action.
        #[command(subcommand)]
        action: SnapshotAction,
    },

    /// Print binary version, protocol version, and Firecracker pin.
    Version,
}

/// `m80 config` sub-actions.
#[derive(Debug, Subcommand)]
pub enum ConfigAction {
    /// Show the effective merged configuration.
    Show,
}

/// `m80 snapshot` sub-actions.
#[derive(Debug, Subcommand)]
pub enum SnapshotAction {
    /// Capture a running VM's state into a snapshot directory.
    ///
    /// v0.1 limitation: capture requires the VM to be launched in the same
    /// process (same binary invocation). Out-of-process capture — where
    /// `m80 snapshot capture <vm-id>` contacts a separately-launched VM —
    /// requires IPC and is deferred to v0.2 (same gap as `m80 exec`).
    ///
    /// Writes vm.snap and mem.snap to the destination directory.
    Capture {
        /// VM id of the running VM to capture.
        vm_id: String,

        /// Directory to write the snapshot files into (must not exist yet,
        /// or must be empty).
        #[arg(long, value_name = "DIR")]
        store_root: PathBuf,
    },
}

/// Parse `noegress` | `outbound` into [`NetworkPolicy`].
///
/// Case-insensitive to be shell-friendly.
fn parse_network_policy(s: &str) -> Result<NetworkPolicy, String> {
    match s.to_ascii_lowercase().as_str() {
        "noegress" => Ok(NetworkPolicy::NoEgress),
        "outbound" => Ok(NetworkPolicy::AllowOutbound { exceptions: vec![] }),
        other => Err(format!(
            "unknown network policy '{other}'; expected: noegress | outbound"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_noegress() {
        let p = parse_network_policy("noegress").unwrap();
        assert_eq!(p, NetworkPolicy::NoEgress);
    }

    #[test]
    fn parse_noegress_upper() {
        let p = parse_network_policy("NoEgress").unwrap();
        assert_eq!(p, NetworkPolicy::NoEgress);
    }

    #[test]
    fn parse_outbound() {
        let p = parse_network_policy("outbound").unwrap();
        assert!(matches!(p, NetworkPolicy::AllowOutbound { .. }));
    }

    #[test]
    fn parse_unknown_policy_fails() {
        let err = parse_network_policy("wireguard").unwrap_err();
        assert!(err.contains("unknown network policy"), "got: {err}");
    }
}
