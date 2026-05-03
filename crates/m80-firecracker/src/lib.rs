//! VM lifecycle state machine: composes the m80 foundation crates into a
//! launchable sandbox. The fat consumer; itself thin in business logic.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epics `m80-t01` (lifecycle), `m80-19i`
//! (concurrency/admission), `m80-ynh` (cleanup/drain), `m80-4ef` (errors),
//! `m80-v7t` (configuration).
//!
//! # Module layout
//!
//! - [`backend`] — `Backend::new` / `admit` / `show_effective_config` /
//!   `recover_stale_run_root`; the admission semaphore lives here.
//! - [`launch`] — `Sandbox::launch` and the 12 numbered phase functions
//!   that compose the preboot pipeline.
//! - [`lifecycle`] — `RunningSandbox::{vm_id, exec, stop, force_kill}`
//!   and `StoppedSandbox::{run_dir, extract_changes, delete,
//!   preserve_for_triage}`.
//! - [`runroot`] — per-VM directory layout, `ownership.lock`, the
//!   `LeaseGuard` RAII helper, `recover_stale_run_root` walk.
//! - [`config`] — five-layer merge (defaults → /etc/m80 → ~/.config/m80
//!   → env M80_* → flags), produces an `EffectiveConfig` with each
//!   field tagged by source.
//! - [`error`] — `FcError` variant set.
//! - [`types`] — public type definitions (`Backend`, `Sandbox`,
//!   `RunningSandbox`, `StoppedSandbox`, `BackendConfig`, etc.) plus
//!   crate-internal types (`StoragePrep`, `RealizedNetwork`, the
//!   `Semaphore` aliases).

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
