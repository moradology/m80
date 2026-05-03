//! VM lifecycle state machine: composes the m80 foundation crates into a
//! launchable sandbox. The fat consumer; itself thin in business logic.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epics `m80-t01` (lifecycle), `m80-19i`
//! (concurrency/admission), `m80-ynh` (cleanup/drain), `m80-4ef` (errors),
//! `m80-v7t` (configuration).

#![deny(missing_docs)]

// =====================================================================
// Module declarations
// =====================================================================

mod backend;
mod config;
mod error;
mod launch;
mod lifecycle;
mod runroot;
mod types;

// =====================================================================
// Public re-exports — the full public surface
// =====================================================================

// Error sum.
pub use error::FcError;

// State types.
pub use types::{
    Backend, BackendConfig, CgroupMode, ConfigSource, EffectiveConfig, EffectiveField,
    RunningSandbox, Sandbox, SandboxConfig, StoppedSandbox,
};

// NetworkPolicy (shared intent type from m80-net-mode).
pub use m80_net_mode::NetworkPolicy;

// Wire types from m80-proto — re-exported so callers don't need a direct
// m80-proto dep for request/response shapes.
pub use m80_proto::{ExecRequest, ExecResponse, ExecStatus, ExecTiming};

// Config helpers for m80-cli.
pub use config::{backend_config_from_effective, load as load_config};

// ChangeSet re-exported because StoppedSandbox::extract_changes returns it.
pub use m80_storage::ChangeSet;
