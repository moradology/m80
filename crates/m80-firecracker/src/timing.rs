//! Per-phase timing emission for the bench harness.
//!
//! Each phase boundary in `launch::launch`, `RunningSandbox::exec`, and
//! `RunningSandbox::stop` emits a single line on stderr when
//! `M80_PHASE_TRACE=1` is set. Format is greppable and key=value parseable:
//!
//! ```text
//! M80_PHASE name=phase_3_storage_prep vm_id=vm-1234-5678 elapsed_us=12345
//! ```
//!
//! The bench script (`scripts/bench-cold-launch.sh`) parses these into a
//! long-format CSV so per-phase contributions to total wallclock can be
//! attributed without re-running.
//!
//! No-op when the env var is unset to keep production stderr quiet.

use std::time::{Duration, Instant};

/// Whether `M80_PHASE_TRACE=1` is set in the environment.
fn enabled() -> bool {
    std::env::var("M80_PHASE_TRACE")
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// Emit one timing event to stderr (no-op unless `M80_PHASE_TRACE=1`).
pub(crate) fn phase_event(name: &str, vm_id: &str, elapsed: Duration) {
    if !enabled() {
        return;
    }
    eprintln!(
        "M80_PHASE name={} vm_id={} elapsed_us={}",
        name,
        vm_id,
        elapsed.as_micros()
    );
}

/// Run a closure and emit a phase event with its elapsed time. Returns
/// the closure's result so call sites read like the unwrapped call:
///
/// ```ignore
/// let storage = phase("phase_3_storage_prep", &vm_id, || {
///     phase_3_storage_prep(...)
/// })?;
/// ```
pub(crate) fn phase<T, F: FnOnce() -> T>(name: &str, vm_id: &str, f: F) -> T {
    let t = Instant::now();
    let out = f();
    phase_event(name, vm_id, t.elapsed());
    out
}
