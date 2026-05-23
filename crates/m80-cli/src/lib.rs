//! `m80-cli` library surface.
//!
//! Declares all modules so integration tests (which link the lib target)
//! can use `Cli::try_parse_from` without spawning a subprocess.
//!
//! The `main.rs` binary entry point imports from here.

#![deny(missing_docs)]

pub mod args;
pub mod runner;

mod cmds;
mod cmds_walk;
mod config;
mod errors;
mod install_state;
mod json;
mod profile;
mod release;
mod release_asset_index;
mod release_freshness;
mod release_policy;
mod release_urls;
mod request_id;

pub use args::{
    BugReportArgs, Cli, Cmd, ConfigAction, EgressMode, ImageAction, ImageBuildArgs, ImageGcArgs,
    ImageKindArg, InstallArgs, InstallCleanupArgs, InstallStatusArgs, OverlayCloneModeArg,
    QuickstartArgs, TemplateAction, TemplateBuildArgs, WarmAction, WarmEnableArgs, WritebackMode,
};

/// Test-only synchronization for process-wide environment mutation.
#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::Mutex;

    /// Serializes tests that mutate PATH or downloader/verifier environment.
    pub(crate) static PROCESS_ENV_LOCK: Mutex<()> = Mutex::new(());
}
