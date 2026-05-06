//! Clap-derive argument types for the `m80` CLI.
//!
//! Every subcommand's fields map 1:1 to the fields of the original
//! type-pinning `Subcommand` enum in `main.rs`.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// The `m80` command-line tool.
///
/// Run a process with a constrained view of the host: selected filesystem
/// visibility, bounded egress, and explicit writeback behavior.
#[derive(Debug, Parser)]
#[command(
    name = "m80",
    about = "Run a process with a constrained view of the host",
    long_about = None
)]
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
    /// Run one process inside an m80 sandbox.
    ///
    /// Pipe mode preserves stdout, stderr, and the guest exit code. The
    /// requested program must exist in the selected guest profile or inside
    /// the visible workspace; m80 does not execute host binaries, pull OCI
    /// images, or install packages implicitly.
    Run {
        /// Runtime image/profile name.
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,

        /// Host workspace directory to mount into the guest.
        #[arg(long, value_name = "PATH")]
        workspace: Option<PathBuf>,

        /// Working directory for the process inside the guest.
        #[arg(long, value_name = "PATH")]
        cwd: Option<String>,

        /// Environment override in KEY=VAL form. May be repeated.
        #[arg(long = "env", value_name = "KEY=VAL")]
        env: Vec<String>,

        /// Copy one named host environment variable into the guest. May be repeated.
        #[arg(long = "secret-env", value_name = "KEY")]
        secret_env: Vec<String>,

        /// Read host stdin fully and send it to the guest process.
        #[arg(long)]
        stdin: bool,

        /// Egress policy for this process.
        #[arg(long, value_enum, default_value = "outbound")]
        egress: EgressMode,

        /// Hostname allowed by the outbound egress policy. Deferred to v0.2.
        #[arg(long = "allow-host", value_name = "HOST")]
        allow_host: Vec<String>,

        /// CIDR allowed by the outbound egress policy. Deferred to v0.2.
        #[arg(long = "allow-cidr", value_name = "CIDR")]
        allow_cidr: Vec<String>,

        /// Host config file mount. Deferred to v0.2.
        #[arg(long = "mount-config", value_name = "HOST:GUEST[:ro]")]
        mount_config: Vec<String>,

        /// Scratch overlay size in bytes.
        #[arg(long, value_name = "BYTES")]
        scratch_size: Option<u64>,

        /// Workspace writeback policy.
        #[arg(long, value_enum, default_value = "never")]
        writeback: WritebackMode,

        /// Preserve sandbox state after failure. Deferred to v0.2.
        #[arg(long)]
        keep_on_failure: bool,

        /// Allocate a terminal stream instead of separated stdout/stderr.
        #[arg(short = 't', long = "tty")]
        tty: bool,

        /// Keep stdin interactive for terminal mode.
        #[arg(short = 'i')]
        interactive: bool,

        /// Lease a ready slot from the explicit warm owner instead of cold booting.
        #[arg(long)]
        warm: bool,

        /// Program and arguments to run inside the selected guest profile.
        #[arg(last = true, required = true, value_name = "ARGV")]
        argv: Vec<String>,
    },

    /// Run the host capability checklist; render a table.
    ///
    /// Exits 0 on full pass, 2 on any check failed.
    Preflight,

    /// Install release artifacts and run the smallest process-wrapper probe.
    ///
    /// Downloads the release tarball, verifies `SHA256SUMS`, installs the
    /// kernel/rootfs/manifest/guestd artifacts, then runs `m80 run -- echo
    /// hello` unless `--no-run` is set.
    Quickstart(QuickstartArgs),

    /// Print a VM's run-dir layout and recorded state.
    Inspect {
        /// VM id.
        vm_id: String,
    },

    /// Dump or follow out-of-band VM diagnostics for one run directory.
    ///
    /// Reads persisted host diagnostics and guest console capture from the
    /// run-root. This never reads or replays the wrapped process stdout/stderr
    /// streams from `m80 run`.
    Logs {
        /// VM id.
        vm_id: String,

        /// Continue polling for new diagnostics records.
        #[arg(long)]
        follow: bool,

        /// Return only records with this opaque request id.
        #[arg(long = "request-id", value_name = "ID")]
        request_id: Option<String>,

        /// Return only records at or after this timestamp.
        #[arg(long, value_name = "RFC3339|UNIX_MS")]
        since: Option<String>,
    },

    /// List all VM run-dirs under the run-root.
    List,

    /// Print host capabilities, effective config, versions, and runtime paths.
    Env,

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

    /// Explicit warm-sandbox owner control.
    Warm {
        /// Warm owner action.
        #[command(subcommand)]
        action: WarmAction,
    },

    /// Print binary version, protocol version, and Firecracker pin.
    Version,
}

/// CLI egress policy selected by `m80 run --egress`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum EgressMode {
    /// No guest NIC and no outbound network.
    None,
    /// NAT-backed outbound network where host preflight can support it.
    Outbound,
}

/// CLI writeback policy selected by `m80 run --writeback`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum WritebackMode {
    /// Discard workspace changes.
    Never,
    /// Write workspace changes back only when the guest exits successfully.
    OnSuccess,
    /// Write workspace changes back even when the guest exits non-zero.
    Always,
}

/// `m80 warm` sub-actions.
#[derive(Debug, Subcommand)]
pub enum WarmAction {
    /// Start the explicit resident warm owner.
    Enable(WarmEnableArgs),
    /// Report the current owner state.
    Status {
        /// Runtime image/profile expected by this caller.
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
    },
    /// Stop accepting leases and tear down owner-owned slots.
    Drain,
    /// Stop the owner and remove owner-owned state.
    Disable,
}

/// Arguments for `m80 warm enable`.
#[derive(Debug, Args)]
pub struct WarmEnableArgs {
    /// Run the owner in the foreground process.
    #[arg(long)]
    pub foreground: bool,

    /// Start/enable the packaged system service. Deferred to a packaging bead.
    #[arg(long)]
    pub system: bool,

    /// Number of ready slots the owner should keep filled.
    #[arg(long, value_name = "N")]
    pub size: usize,

    /// Egress policy baked into warm slots.
    #[arg(long, value_enum, default_value = "outbound")]
    pub egress: EgressMode,

    /// Runtime image/profile name for warm slots.
    #[arg(long, value_name = "NAME")]
    pub profile: Option<String>,
}

/// Arguments for `m80 quickstart`.
#[derive(Debug, Args)]
pub struct QuickstartArgs {
    /// Release artifact tarball URL.
    #[arg(long = "artifact-url", value_name = "URL")]
    pub artifact_url: String,

    /// Artifact install directory. Defaults to M80_ARTIFACT_DIR or /opt/m80/artifacts.
    #[arg(long = "artifact-dir", value_name = "PATH")]
    pub artifact_dir: Option<PathBuf>,

    /// Run-root directory. Defaults to M80_RUN_ROOT or /var/run/m80.
    #[arg(long = "run-root", value_name = "PATH")]
    pub run_root: Option<PathBuf>,

    /// Install and verify artifacts but do not run echo.
    #[arg(long = "no-run")]
    pub no_run: bool,
}

/// `m80 config` sub-actions.
#[derive(Debug, Subcommand)]
pub enum ConfigAction {
    /// Show the effective merged configuration.
    Show,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn egress_mode_has_expected_values() {
        let values: Vec<_> = EgressMode::value_variants()
            .iter()
            .map(|v| v.to_possible_value().unwrap().get_name().to_owned())
            .collect();
        assert_eq!(values, ["none", "outbound"]);
    }

    #[test]
    fn writeback_mode_has_expected_values() {
        let values: Vec<_> = WritebackMode::value_variants()
            .iter()
            .map(|v| v.to_possible_value().unwrap().get_name().to_owned())
            .collect();
        assert_eq!(values, ["never", "on-success", "always"]);
    }
}
