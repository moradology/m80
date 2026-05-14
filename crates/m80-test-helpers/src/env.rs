//! Environment variable isolation helpers for integration tests.
//!
//! Tests that read from environment variables must run with a known env state.
//! Use [`env_lock`] to serialize env-touching tests, and [`EnvRestore`] to
//! automatically restore captured values on drop (even on panic).

use std::ffi::OsString;
use std::sync::{Mutex, OnceLock};

/// Return the process-wide mutex that env-touching tests must hold.
///
/// All tests that set or unset environment variables should lock this mutex
/// for the duration of the test to avoid races with parallel test threads.
pub fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// RAII guard that restores a set of environment variables on drop.
///
/// Capture the current values of `keys` with [`EnvRestore::capture`], then
/// mutate the environment freely. When the guard drops — including on panic —
/// all captured keys are restored to their original values.
pub struct EnvRestore {
    values: Vec<(&'static str, Option<OsString>)>,
}

impl EnvRestore {
    /// Snapshot the current value (or absence) of each key in `keys`.
    #[must_use]
    pub fn capture(keys: &[&'static str]) -> Self {
        Self {
            values: keys
                .iter()
                .map(|key| (*key, std::env::var_os(key)))
                .collect(),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (key, value) in &self.values {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}
