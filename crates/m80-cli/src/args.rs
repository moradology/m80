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
#[command(version = crate::release::DISPLAY_VERSION)]
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

        /// Scratch overlay size in bytes.
        #[arg(long, value_name = "BYTES")]
        scratch_size: Option<u64>,

        /// Overlay template clone policy.
        #[arg(long, value_enum, default_value = "byte-copy")]
        overlay_clone_mode: OverlayCloneModeArg,

        /// Number of vCPUs assigned to the cold-booted VM.
        #[arg(long, value_name = "N")]
        vcpu_count: Option<u32>,

        /// Guest memory assigned to the cold-booted VM, in MiB.
        #[arg(long, value_name = "MIB")]
        mem_size_mib: Option<u32>,

        /// Workspace writeback policy.
        #[arg(long, value_enum, default_value = "never")]
        writeback: WritebackMode,

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

    /// Install an explicit artifact tarball for operator/test overrides.
    ///
    /// The normal Linux first-run path is the release `install.sh`. This
    /// command is for local fixtures or an explicitly pinned tarball that
    /// matches the running m80 binary; it verifies `SHA256SUMS`, installs the
    /// kernel/rootfs/manifest/guestd artifacts, then runs `m80 run -- echo
    /// hello` unless `--no-run` is set. Legacy artifact-only install repairs
    /// are documented in docs/behaviors/release/legacy-quickstart-hard-cutover.md.
    Quickstart(QuickstartArgs),

    /// Install or plan a release bundle.
    ///
    /// This validates the user-facing installer inputs and release identity.
    /// `--dry-run` is side-effect-free; non-dry-run currently supports local
    /// `file://` bundle layout copies without active-pointer finalization.
    Install(InstallArgs),

    /// Show the installed release selected by the local host configuration.
    #[command(name = "install-status")]
    InstallStatus(InstallStatusArgs),

    /// Check or update the installed release.
    ///
    /// `--check` is read-only and reports whether the active install is
    /// current against bounded freshness metadata. Mutating update apply is a
    /// separate release transaction.
    Update(UpdateArgs),

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

    /// Manage content-addressed image-store artifacts.
    Image {
        /// Image-store action.
        #[command(subcommand)]
        action: ImageAction,
    },

    /// Manage snapshot-template store artifacts.
    Template {
        /// Snapshot-template action.
        #[command(subcommand)]
        action: TemplateAction,
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

/// CLI overlay template clone policy selected by `m80 run --overlay-clone-mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OverlayCloneModeArg {
    /// Probe the run-root filesystem and select reflink or byte-copy before cloning.
    Auto,
    /// Require byte-copy with reflinks disabled.
    ByteCopy,
    /// Require reflink/CoW clone semantics.
    Reflink,
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

/// `m80 image` sub-actions.
#[derive(Debug, Subcommand)]
pub enum ImageAction {
    /// Build or import an image artifact into an image store.
    Build(ImageBuildArgs),
    /// Report or execute image-store garbage collection.
    Gc(ImageGcArgs),
    /// List image artifacts in a store.
    List(ImageStoreArgs),
    /// Show one image digest.
    Show(ImageDigestArgs),
    /// Remove one image digest after reference checks pass.
    Rm(ImageRmArgs),
    /// Verify stored bytes and metadata for one image digest.
    Verify(ImageDigestArgs),
}

/// Arguments for `m80 image build`.
#[derive(Debug, Args)]
pub struct ImageBuildArgs {
    /// Operator label echoed in command output; images are stored by digest.
    pub name: String,

    /// Source directory to build, or pre-built artifact file to import.
    #[arg(long, value_name = "PATH")]
    pub source: PathBuf,

    /// Image-store root to write.
    #[arg(long = "out", value_name = "PATH")]
    pub out: PathBuf,

    /// Filesystem artifact kind.
    #[arg(long, value_enum, default_value = "erofs")]
    pub kind: ImageKindArg,
}

/// Arguments for `m80 image gc`.
#[derive(Debug, Args)]
pub struct ImageGcArgs {
    /// Image-store root.
    #[arg(long, value_name = "PATH", default_value = "/var/lib/m80-images")]
    pub store: PathBuf,

    /// Snapshot-template store root checked for committed image references.
    #[arg(
        long = "template-store",
        value_name = "PATH",
        default_value = "/var/lib/m80/templates"
    )]
    pub template_store: PathBuf,

    /// Lowercase sha256 image digest to retain. May be repeated.
    #[arg(long = "keep", value_name = "DIGEST")]
    pub keep: Vec<String>,

    /// Newline-delimited lowercase sha256 digests to retain.
    #[arg(long = "pin-file", value_name = "PATH")]
    pub pin_file: Option<PathBuf>,

    /// Do not delete artifacts newer than this age, for example 30s, 10m, 6h, or 7d.
    #[arg(long = "min-age", value_name = "DURATION")]
    pub min_age: Option<String>,

    /// Delete candidates. Omitted means report-only dry run.
    #[arg(long)]
    pub execute: bool,
}

/// Shared image-store path argument.
#[derive(Debug, Args)]
pub struct ImageStoreArgs {
    /// Image-store root.
    #[arg(long, value_name = "PATH", default_value = "/var/lib/m80-images")]
    pub store: PathBuf,
}

/// Arguments for image commands that operate on one digest.
#[derive(Debug, Args)]
pub struct ImageDigestArgs {
    /// Lowercase sha256 image digest.
    pub digest: String,

    /// Image-store root.
    #[arg(long, value_name = "PATH", default_value = "/var/lib/m80-images")]
    pub store: PathBuf,
}

/// Arguments for `m80 image rm`.
#[derive(Debug, Args)]
pub struct ImageRmArgs {
    /// Lowercase sha256 image digest.
    pub digest: String,

    /// Image-store root.
    #[arg(long, value_name = "PATH", default_value = "/var/lib/m80-images")]
    pub store: PathBuf,

    /// Snapshot-template store root checked for references before removal.
    #[arg(
        long = "template-store",
        value_name = "PATH",
        default_value = "/var/lib/m80/templates"
    )]
    pub template_store: PathBuf,
}

/// CLI value for image artifact kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ImageKindArg {
    /// Erofs read-only filesystem image.
    Erofs,
    /// Ext4 filesystem image.
    Ext4,
}

impl From<ImageKindArg> for m80_image_store::ImageKind {
    fn from(value: ImageKindArg) -> Self {
        match value {
            ImageKindArg::Erofs => Self::Erofs,
            ImageKindArg::Ext4 => Self::Ext4,
        }
    }
}

/// `m80 template` sub-actions.
#[derive(Debug, Subcommand)]
pub enum TemplateAction {
    /// Build a snapshot template from a BootSpec.
    Build(TemplateBuildArgs),
    /// List templates in a store.
    List(TemplateStoreArgs),
    /// Show one committed template manifest.
    Show(TemplateFingerprintArgs),
    /// Prune invalidated templates when a BootSpec is supplied.
    Prune(TemplatePruneArgs),
    /// Remove one unpinned template.
    Rm(TemplateFingerprintArgs),
}

/// Arguments for `m80 template build`.
#[derive(Debug, Args)]
pub struct TemplateBuildArgs {
    /// Operator label echoed in command output; templates are stored by fingerprint.
    pub name: String,

    /// BootSpec YAML or JSON path.
    #[arg(long = "boot-spec", value_name = "PATH")]
    pub boot_spec: PathBuf,
}

/// Shared snapshot-template store path argument.
#[derive(Debug, Args)]
pub struct TemplateStoreArgs {
    /// Snapshot-template store root.
    #[arg(long, value_name = "PATH", default_value = "/var/lib/m80/templates")]
    pub store: PathBuf,
}

/// Arguments for commands operating on one template fingerprint.
#[derive(Debug, Args)]
pub struct TemplateFingerprintArgs {
    /// Lowercase 64-character template fingerprint.
    pub fingerprint: String,

    /// Snapshot-template store root.
    #[arg(long, value_name = "PATH", default_value = "/var/lib/m80/templates")]
    pub store: PathBuf,
}

/// Arguments for `m80 template prune`.
#[derive(Debug, Args)]
pub struct TemplatePruneArgs {
    /// Snapshot-template store root.
    #[arg(long, value_name = "PATH", default_value = "/var/lib/m80/templates")]
    pub store: PathBuf,

    /// BootSpec YAML or JSON used to compute the current live template inputs.
    #[arg(long = "boot-spec", value_name = "PATH")]
    pub boot_spec: Option<PathBuf>,
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
    /// Operator/test artifact tarball URL matching this m80 binary.
    #[arg(long = "artifact-url", value_name = "URL")]
    pub artifact_url: String,

    /// Artifact install directory. Defaults to M80_ARTIFACT_DIR or /opt/m80/artifacts.
    #[arg(long = "artifact-dir", value_name = "PATH")]
    pub artifact_dir: Option<PathBuf>,

    /// Run-root directory. Defaults to M80_RUN_ROOT or /var/run/m80.
    #[arg(long = "run-root", value_name = "PATH")]
    pub run_root: Option<PathBuf>,

    /// Runtime profile directory override for install-root fixtures.
    #[arg(long = "profile-dir", value_name = "PATH", hide = true)]
    pub profile_dir: Option<PathBuf>,

    /// m80 config file path override for install-root fixtures.
    #[arg(long = "config-path", value_name = "PATH", hide = true)]
    pub config_path: Option<PathBuf>,

    /// Install and verify artifacts but do not run echo.
    #[arg(long = "no-run")]
    pub no_run: bool,
}

/// Arguments for `m80 install`.
#[derive(Debug, Args)]
#[command(override_usage = "m80 install [OPTIONS] <--release-tag <TAG>|--bundle-url <URL>>")]
pub struct InstallArgs {
    /// Pinned release tag to install, for example v0.1.0.
    #[arg(
        long = "release-tag",
        value_name = "TAG",
        conflicts_with_all = ["bundle_url", "bootstrap_tag"]
    )]
    pub release_tag: Option<String>,

    /// Explicit release bundle URL to install.
    #[arg(
        long = "bundle-url",
        value_name = "URL",
        conflicts_with_all = ["release_tag", "bootstrap_tag"]
    )]
    pub bundle_url: Option<String>,

    /// Concrete release tag selected by the stable bootstrapper.
    #[arg(
        long = "bootstrap-tag",
        value_name = "TAG",
        hide = true,
        conflicts_with_all = ["release_tag", "bundle_url"]
    )]
    pub bootstrap_tag: Option<String>,

    /// Install root to plan or populate. Defaults to /opt/m80.
    #[arg(long = "install-root", value_name = "PATH", default_value = "/opt/m80")]
    pub install_root: PathBuf,

    /// Print the install plan without touching host state.
    #[arg(long = "dry-run")]
    pub dry_run: bool,

    /// Remove a stale install-state lock whose owner process is no longer running.
    #[arg(long = "repair-stale-install-lock")]
    pub repair_stale_install_lock: bool,
}

/// Arguments for `m80 install-status`.
#[derive(Debug, Args)]
pub struct InstallStatusArgs {
    /// Install root to inspect. Defaults to /opt/m80.
    #[arg(long, value_name = "PATH", default_value = "/opt/m80")]
    pub install_root: PathBuf,

    /// Profile override to inspect instead of the effective default profile.
    #[arg(long, value_name = "NAME")]
    pub profile: Option<String>,
}

/// Arguments for `m80 update`.
#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Report update status without changing install-root state.
    #[arg(long)]
    pub check: bool,

    /// Install root to inspect. Defaults to /opt/m80.
    #[arg(long, value_name = "PATH", default_value = "/opt/m80")]
    pub install_root: PathBuf,

    /// Profile override to inspect instead of the effective default profile.
    #[arg(long, value_name = "NAME")]
    pub profile: Option<String>,

    /// Read a bounded freshness status artifact from this path. When
    /// --latest-status-url is also set, this path is used as the read-only
    /// fallback cache if the URL is unavailable.
    #[arg(long = "latest-status", value_name = "PATH")]
    pub latest_status: Option<PathBuf>,

    /// Fetch a bounded freshness status artifact from this URL.
    #[arg(long = "latest-status-url", value_name = "URL")]
    pub latest_status_url: Option<String>,

    /// Test fixture override for the system config path.
    #[arg(long = "config-path", value_name = "PATH", hide = true)]
    pub config_path: Option<PathBuf>,

    /// Test fixture override for the system profile directory.
    #[arg(long = "profile-dir", value_name = "PATH", hide = true)]
    pub profile_dir: Option<PathBuf>,
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
