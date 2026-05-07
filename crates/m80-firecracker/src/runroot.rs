//! Run-root layout helpers: per-VM directory paths, `ownership.lock`,
//! recovery scan.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{ConfigError, FcError};

/// Contents of the `ownership.lock` file written by the process that created
/// this run-dir.
#[derive(Debug)]
struct OwnershipRecord {
    pid: u32,
}

/// Liveness classification for one run directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunDirLiveness {
    /// The ownership marker points at a live host process.
    Live,
    /// No ownership marker exists, or it points at a dead process.
    Dead,
    /// An ownership marker exists but cannot be parsed.
    Ambiguous,
}

/// File name of the per-VM ownership marker. Public because m80-cli walks
/// run-dirs externally and needs the same constant for liveness checks.
pub const OWNERSHIP_LOCK: &str = "ownership.lock";

/// Current Unix time in milliseconds.
pub(crate) fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Write the `ownership.lock` file and return a guard that removes it on drop.
pub(crate) fn write_ownership_lock(run_dir: &Path) -> Result<LeaseGuard, FcError> {
    let lock_path = run_dir.join(OWNERSHIP_LOCK);

    // Check for a pre-existing lock from another live process.
    if lock_path.exists() {
        let existing = read_ownership_lock(&lock_path).map_err(|()| {
            FcError::Config(ConfigError::Other(format!(
                "run-dir {} has an ambiguous ownership lock",
                run_dir.display()
            )))
        })?;
        if pid_is_alive(existing.pid) {
            return Err(FcError::Config(ConfigError::Other(format!(
                "run-dir {} already owned by pid {}",
                run_dir.display(),
                existing.pid
            ))));
        }
    }

    let pid = std::process::id();
    let started_at = unix_ms_now();
    let content = format!("pid={pid}\nstarted_at={started_at}\n");
    std::fs::write(&lock_path, content.as_bytes()).map_err(FcError::Io)?;

    Ok(LeaseGuard { lock_path })
}

/// Parse an `ownership.lock` file.
fn read_ownership_lock(lock_path: &Path) -> Result<OwnershipRecord, ()> {
    let content = std::fs::read_to_string(lock_path).map_err(|_| ())?;
    let mut pid = None;
    for line in content.lines() {
        if let Some(val) = line.strip_prefix("pid=") {
            pid = val.parse::<u32>().ok();
        }
    }
    match pid {
        Some(pid) => Ok(OwnershipRecord { pid }),
        None => Err(()),
    }
}

/// Returns `true` if `/proc/<pid>` exists (process is alive on Linux).
pub(crate) fn pid_is_alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

/// RAII guard: removes `ownership.lock` when dropped.
///
/// **Naming gotcha:** when binding a `LeaseGuard` you want to keep alive
/// for the rest of a function, never use a bare `_` name (`let _lease =
/// write_ownership_lock(...)?`). Rust drops a bare-`_`-bound value at the
/// end of the let-statement, immediately removing the lock file before
/// the rest of the function runs. Use a name with a suffix
/// (`_lease_guard`) so the binding lives until the end of the enclosing
/// scope.
pub(crate) struct LeaseGuard {
    lock_path: PathBuf,
}

impl Drop for LeaseGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

impl std::fmt::Debug for LeaseGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LeaseGuard")
            .field("lock_path", &self.lock_path)
            .finish()
    }
}

/// Inspect a run-dir's ownership lock.
pub(crate) fn run_dir_liveness(run_dir: &Path) -> RunDirLiveness {
    let lock_path = run_dir.join(OWNERSHIP_LOCK);
    if !lock_path.exists() {
        return RunDirLiveness::Dead;
    }
    match read_ownership_lock(&lock_path) {
        Ok(record) if pid_is_alive(record.pid) => RunDirLiveness::Live,
        Ok(_) => RunDirLiveness::Dead,
        Err(()) => RunDirLiveness::Ambiguous,
    }
}
