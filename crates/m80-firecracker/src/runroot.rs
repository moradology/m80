//! Run-root layout helpers: per-VM directory paths, `ownership.lock`,
//! recovery scan.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::FcError;

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
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    u64::try_from(ms).unwrap_or(u64::MAX)
}

/// Write the `ownership.lock` file and return a guard that removes it on drop.
pub(crate) fn write_ownership_lock(run_dir: &Path) -> Result<LeaseGuard, FcError> {
    let lock_path = run_dir.join(OWNERSHIP_LOCK);

    // Check for a pre-existing lock from another live process.
    if lock_path.exists() {
        let existing =
            read_ownership_lock(&lock_path).map_err(|()| FcError::RunDirOwnershipAmbiguous {
                run_dir: run_dir.to_path_buf(),
            })?;
        if pid_is_alive(existing.pid) {
            return Err(FcError::RunDirAlreadyOwned {
                run_dir: run_dir.to_path_buf(),
                pid: existing.pid,
            });
        }
    }

    let pid = std::process::id();
    let started_at = unix_ms_now();
    let content = format!("pid={pid}\nstarted_at={started_at}\n");
    std::fs::write(&lock_path, content.as_bytes()).map_err(|source| FcError::PathIo {
        path: lock_path.clone(),
        source,
    })?;

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
    pid.map(|pid| OwnershipRecord { pid }).ok_or(())
}

/// Returns `true` if `/proc/<pid>` exists and is not a zombie/dead task.
pub(crate) fn pid_is_alive(pid: u32) -> bool {
    let proc_dir = Path::new("/proc").join(pid.to_string());
    let stat_path = proc_dir.join("stat");
    match std::fs::read_to_string(stat_path) {
        Ok(stat) => match proc_stat_state(&stat) {
            Some('Z' | 'X') => false,
            Some(_) => true,
            None => proc_dir.exists(),
        },
        Err(_) => proc_dir.exists(),
    }
}

fn proc_stat_state(stat: &str) -> Option<char> {
    let (_comm, after_comm) = stat.rsplit_once(") ")?;
    after_comm.chars().next()
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
#[derive(Debug)]
pub(crate) struct LeaseGuard {
    lock_path: PathBuf,
}

impl Drop for LeaseGuard {
    fn drop(&mut self) {
        match std::fs::remove_file(&self.lock_path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                tracing::warn!(
                    path = %self.lock_path.display(),
                    error = %err,
                    "failed to remove run-dir ownership lock"
                );
            }
        }
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

#[cfg(test)]
mod tests {
    use super::proc_stat_state;

    #[test]
    fn proc_stat_state_parses_running_task() {
        assert_eq!(proc_stat_state("123 (firecracker) S 1 2 3"), Some('S'));
    }

    #[test]
    fn proc_stat_state_parses_zombie_task() {
        assert_eq!(proc_stat_state("123 (firecracker) Z 1 2 3"), Some('Z'));
    }

    #[test]
    fn proc_stat_state_uses_last_comm_close_paren() {
        assert_eq!(proc_stat_state("123 (name with ) char) R 1 2 3"), Some('R'));
    }

    #[test]
    fn proc_stat_state_rejects_malformed_stat() {
        assert_eq!(proc_stat_state("123 firecracker S 1 2 3"), None);
    }
}
