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
mod json;
mod profile;
mod request_id;

pub use args::{
    Cli, Cmd, ConfigAction, EgressMode, ImageAction, ImageBuildArgs, ImageDigestArgs, ImageGcArgs,
    ImageKindArg, ImageRmArgs, ImageStoreArgs, OverlayCloneModeArg, QuickstartArgs, TemplateAction,
    TemplateBuildArgs, TemplateFingerprintArgs, TemplatePruneArgs, TemplateStoreArgs, WarmAction,
    WarmEnableArgs, WritebackMode,
};
