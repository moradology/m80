use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use m80_firecracker::{ConfigError, FcError};
use serde::{Deserialize, Serialize};

use super::super::InstallPlan;

const LOCK_FILE_NAME: &str = ".install-state.lock";

pub(super) struct InstallStateLock {
    path: PathBuf,
    record: InstallStateLockRecord,
    _file: File,
}

impl Drop for InstallStateLock {
    fn drop(&mut self) {
        if read_lock_record(&self.path).as_ref() == Some(&self.record) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
struct InstallStateLockRecord {
    owner_pid: u32,
    command: String,
    resolved_tag: Option<String>,
    started_at_unix_seconds: u64,
    owner_proc_start_ticks: u64,
}

pub(super) fn acquire_install_state_lock(
    plan: &InstallPlan,
    install_root: &Path,
) -> Result<InstallStateLock, FcError> {
    let path = install_root.join(LOCK_FILE_NAME);
    match create_lock_file(&path, plan) {
        Ok((file, record)) => Ok(InstallStateLock {
            path,
            record,
            _file: file,
        }),
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
            handle_existing_lock(path, plan)
        }
        Err(source) => Err(FcError::PathIo { path, source }),
    }
}

fn handle_existing_lock(path: PathBuf, plan: &InstallPlan) -> Result<InstallStateLock, FcError> {
    let record = read_lock_record(&path);
    if plan.repair_stale_install_lock {
        let Some(record) = record.as_ref() else {
            return Err(lock_held_error(&path, None));
        };
        if lock_owner_matches_record(record) {
            return Err(lock_held_error(&path, Some(record)));
        }
        fs::remove_file(&path).map_err(|source| FcError::PathIo {
            path: path.clone(),
            source,
        })?;
        return match create_lock_file(&path, plan) {
            Ok((file, record)) => Ok(InstallStateLock {
                path,
                record,
                _file: file,
            }),
            Err(source) => Err(FcError::PathIo { path, source }),
        };
    }
    Err(lock_held_error(&path, record.as_ref()))
}

fn create_lock_file(
    path: &Path,
    plan: &InstallPlan,
) -> Result<(File, InstallStateLockRecord), io::Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let record = InstallStateLockRecord {
        owner_pid: std::process::id(),
        command: std::env::args().collect::<Vec<_>>().join(" "),
        resolved_tag: plan.source.release_tag.clone(),
        started_at_unix_seconds: current_unix_seconds(),
        owner_proc_start_ticks: proc_start_ticks(std::process::id())
            .ok_or_else(|| io::Error::other("failed to read owner process start time"))?,
    };
    let body = serde_json::to_vec_pretty(&record).map_err(io::Error::other)?;
    file.write_all(&body)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok((file, record))
}

fn read_lock_record(path: &Path) -> Option<InstallStateLockRecord> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn lock_owner_matches_record(record: &InstallStateLockRecord) -> bool {
    let Some(current_start_ticks) = proc_start_ticks(record.owner_pid) else {
        return false;
    };
    record.owner_proc_start_ticks == current_start_ticks
}

fn proc_start_ticks(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(Path::new("/proc").join(pid.to_string()).join("stat")).ok()?;
    let after_comm = stat.rsplit_once(") ")?.1;
    after_comm.split_whitespace().nth(19)?.parse().ok()
}

fn lock_held_error(path: &Path, record: Option<&InstallStateLockRecord>) -> FcError {
    let owner = record
        .map(|record| {
            format!(
                "owner_pid={} resolved_tag={} started_at_unix_seconds={} command={}",
                record.owner_pid,
                record.resolved_tag.as_deref().unwrap_or("<unavailable>"),
                record.started_at_unix_seconds,
                record.command
            )
        })
        .unwrap_or_else(|| "owner=<unreadable>".to_owned());
    FcError::Config(ConfigError::InvalidValue {
        field: "install.lock",
        reason: format!(
            "install-state lock is held: path={} {owner}; repair_command=m80 install --repair-stale-install-lock ...",
            path.display()
        ),
    })
}

fn current_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
