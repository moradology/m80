//! Thin shim over `m80_firecracker::config` so the CLI doesn't reimplement
//! the merge logic.
//!
//! `m80-firecracker` owns the canonical loader (defaults + /etc/m80 +
//! ~/.config/m80 + env M80_* + flag overrides → `EffectiveConfig`). This
//! module just translates the CLI's flag-override map into the owned
//! shape `m80-firecracker::load_config` expects, calls into it, and
//! exposes the lightweight `resolve_run_root()` fast path the read-only
//! walk subcommands use.
//!
//! Behavior captures: beads `m80-v7t.1` (env-var schema), `m80-v7t.2`
//! (loading order). The schema now lives in
//! `crates/m80-firecracker/src/config.rs`; this file is documentation +
//! convenience surface for the CLI.

use std::collections::HashMap;
use std::path::PathBuf;

use m80_firecracker::{
    backend_config_from_effective, load_config, BackendConfig, EffectiveConfig, FcError,
};
use m80_preflight::Discovery;

/// Load only the merged effective config.
///
/// This path is used before preflight when CLI behavior (for example runtime
/// profile selection) must be known before boot artifacts are discovered.
pub(crate) fn load_effective(flag_overrides: &HashMap<&str, String>) -> Result<EffectiveConfig, FcError> {
    let owned: HashMap<String, String> = flag_overrides
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect();
    load_config(owned)
}

/// Convert an already-loaded effective config into a `BackendConfig`.
pub(crate) fn backend_config(
    discovery: Discovery,
    effective: &EffectiveConfig,
) -> Result<BackendConfig, FcError> {
    backend_config_from_effective(effective, discovery)
}

/// Resolve the effective `run_root` without running preflight.
///
/// Used by the read-only walk commands (`m80 inspect`, `m80 list`) so
/// they pick up the same value `m80 run` and `m80 cleanup` use, but without
/// paying for a full `m80-preflight::run()`.
pub(crate) fn resolve_run_root() -> Result<PathBuf, FcError> {
    let effective = load_effective(&HashMap::new())?;
    let value = effective
        .fields
        .iter()
        .find(|f| f.name == "run_root")
        .map(|f| f.value.as_str())
        .unwrap_or("/var/run/m80");
    Ok(PathBuf::from(value))
}
