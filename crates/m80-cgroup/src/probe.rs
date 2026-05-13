use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{CgroupError, CGROUP_V2_ROOT};

static PROBE_RESULT: OnceLock<CachedProbeResult> = OnceLock::new();

#[cfg(test)]
static HOST_PROBE_READS: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn probe() -> Result<(), CgroupError> {
    probe_once(&PROBE_RESULT, read_host_probe)
}

fn read_host_probe() -> Result<(), CgroupError> {
    #[cfg(test)]
    HOST_PROBE_READS.fetch_add(1, Ordering::SeqCst);

    let mounts = fs::read_to_string("/proc/mounts").map_err(|source| CgroupError::Io {
        path: PathBuf::from("/proc/mounts"),
        source,
    })?;
    probe_mounts(&mounts)
}

#[cfg(test)]
pub(crate) fn host_probe_read_count() -> usize {
    HOST_PROBE_READS.load(Ordering::SeqCst)
}

#[cfg(test)]
pub(crate) fn probe_with_cache<F>(
    cache: &OnceLock<CachedProbeResult>,
    read_mounts: F,
) -> Result<(), CgroupError>
where
    F: FnOnce() -> Result<String, CgroupError>,
{
    probe_once(cache, || {
        let mounts = read_mounts()?;
        probe_mounts(&mounts)
    })
}

fn probe_once<F>(cache: &OnceLock<CachedProbeResult>, probe: F) -> Result<(), CgroupError>
where
    F: FnOnce() -> Result<(), CgroupError>,
{
    cache
        .get_or_init(|| CachedProbeResult::from(probe()))
        .clone()
        .into_result()
}

#[derive(Debug, Clone)]
pub(crate) enum CachedProbeResult {
    Ok,
    UnsupportedHostMode,
    ControllerNotEnabled(&'static str),
    SparseInheritedFile(&'static str),
    InvalidLimit {
        field: &'static str,
        value: String,
    },
    LivePids {
        path: PathBuf,
        pids: String,
    },
    Io {
        path: PathBuf,
        kind: io::ErrorKind,
        message: String,
    },
}

impl CachedProbeResult {
    fn into_result(self) -> Result<(), CgroupError> {
        match self {
            CachedProbeResult::Ok => Ok(()),
            CachedProbeResult::UnsupportedHostMode => Err(CgroupError::UnsupportedHostMode),
            CachedProbeResult::ControllerNotEnabled(controller) => {
                Err(CgroupError::ControllerNotEnabled(controller))
            }
            CachedProbeResult::SparseInheritedFile(filename) => {
                Err(CgroupError::SparseInheritedFile(filename))
            }
            CachedProbeResult::InvalidLimit { field, value } => {
                Err(CgroupError::InvalidLimit { field, value })
            }
            CachedProbeResult::LivePids { path, pids } => Err(CgroupError::LivePids { path, pids }),
            CachedProbeResult::Io {
                path,
                kind,
                message,
            } => Err(CgroupError::Io {
                path,
                source: io::Error::new(kind, message),
            }),
        }
    }
}

impl From<Result<(), CgroupError>> for CachedProbeResult {
    fn from(result: Result<(), CgroupError>) -> Self {
        match result {
            Ok(()) => CachedProbeResult::Ok,
            Err(CgroupError::UnsupportedHostMode) => CachedProbeResult::UnsupportedHostMode,
            Err(CgroupError::ControllerNotEnabled(controller)) => {
                CachedProbeResult::ControllerNotEnabled(controller)
            }
            Err(CgroupError::SparseInheritedFile(filename)) => {
                CachedProbeResult::SparseInheritedFile(filename)
            }
            Err(CgroupError::InvalidLimit { field, value }) => {
                CachedProbeResult::InvalidLimit { field, value }
            }
            Err(CgroupError::LivePids { path, pids }) => CachedProbeResult::LivePids { path, pids },
            Err(CgroupError::Io { path, source }) => CachedProbeResult::Io {
                path,
                kind: source.kind(),
                message: source.to_string(),
            },
        }
    }
}

pub(crate) fn probe_mounts(mounts: &str) -> Result<(), CgroupError> {
    let has_v2 = mounts.lines().any(|line| {
        let mut cols = line.split_whitespace();
        let _dev = cols.next();
        let mount_point = cols.next().unwrap_or("");
        let fs_type = cols.next().unwrap_or("");
        fs_type == "cgroup2" && mount_point == CGROUP_V2_ROOT
    });

    if !has_v2 {
        return Err(CgroupError::UnsupportedHostMode);
    }

    fs::read_to_string(Path::new(CGROUP_V2_ROOT).join("cgroup.subtree_control")).map_err(
        |source| CgroupError::Io {
            path: Path::new(CGROUP_V2_ROOT).join("cgroup.subtree_control"),
            source,
        },
    )?;

    Ok(())
}
