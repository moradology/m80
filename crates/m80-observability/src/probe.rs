use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ObservabilityError;

// Keep these local: `m80-firecracker` depends on `m80-observability`, so this
// crate cannot import layout constants from the orchestrator without a cycle.
const FIRECRACKER_API_SOCKET: &str = "firecracker.sock";
const OWNERSHIP_LOCK: &str = "ownership.lock";
const VSOCK_SOCKET: &str = "vsock.sock";

/// One per-VM probe record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VmProbeRecord {
    /// Health classification.
    pub health: VmHealth,
    /// Path of the run-dir.
    pub run_dir: PathBuf,
    /// VM identifier.
    pub vm_id: String,
    /// Ownership pid read from `ownership.lock`, if present and parseable.
    pub ownership_pid: Option<u32>,
    /// Whether the ownership pid is currently visible under `/proc`.
    pub owner_live: bool,
    /// Whether a Firecracker API socket is visible in the run-dir tree.
    pub api_socket_visible: bool,
    /// Whether a vsock muxer socket is visible in the run-dir tree.
    pub vsock_socket_visible: bool,
    /// Whether `diagnostics.jsonl` exists in the run-dir.
    pub diagnostics_visible: bool,
}

/// Health classification produced by the probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum VmHealth {
    /// Live but with degraded indicators.
    Degraded,
    /// Process tree gone; residue remains.
    Exited,
    /// Process and expected sockets are visible.
    Healthy,
    /// Live but no socket reachability evidence is visible.
    Stuck,
}

/// Probe over a run-root and emit one record per owned VM.
pub fn probe(run_root: &Path) -> Result<Vec<VmProbeRecord>, ObservabilityError> {
    let mut records = Vec::new();
    if !run_root.exists() {
        return Ok(records);
    }

    for entry in std::fs::read_dir(run_root).map_err(|source| ObservabilityError::PathIo {
        path: run_root.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| ObservabilityError::PathIo {
            path: run_root.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if !path.is_dir() || !path.join(OWNERSHIP_LOCK).exists() {
            continue;
        }
        let Some(vm_id) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        records.push(probe_one(vm_id, &path)?);
    }
    records.sort_by(|a, b| a.vm_id.cmp(&b.vm_id));
    Ok(records)
}

fn probe_one(vm_id: &str, run_dir: &Path) -> Result<VmProbeRecord, ObservabilityError> {
    let ownership_pid = read_ownership_pid(run_dir);
    let owner_live = ownership_pid
        .map(|pid| Path::new(&format!("/proc/{pid}")).exists())
        .unwrap_or(false);
    let api_socket_visible = tree_contains_file_name(run_dir, FIRECRACKER_API_SOCKET)?;
    let vsock_socket_visible = tree_contains_file_name(run_dir, VSOCK_SOCKET)?;
    let diagnostics_visible = run_dir.join(crate::DIAGNOSTICS_FILE_NAME).exists();
    let health = classify(owner_live, api_socket_visible, vsock_socket_visible);

    Ok(VmProbeRecord {
        health,
        run_dir: run_dir.to_path_buf(),
        vm_id: vm_id.to_owned(),
        ownership_pid,
        owner_live,
        api_socket_visible,
        vsock_socket_visible,
        diagnostics_visible,
    })
}

fn classify(owner_live: bool, api_socket_visible: bool, vsock_socket_visible: bool) -> VmHealth {
    if !owner_live {
        return VmHealth::Exited;
    }
    match (api_socket_visible, vsock_socket_visible) {
        (true, true) => VmHealth::Healthy,
        (false, false) => VmHealth::Stuck,
        _ => VmHealth::Degraded,
    }
}

fn read_ownership_pid(run_dir: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(run_dir.join(OWNERSHIP_LOCK)).ok()?;
    text.lines().find_map(|line| {
        line.strip_prefix("pid=")
            .and_then(|value| value.trim().parse::<u32>().ok())
    })
}

fn tree_contains_file_name(root: &Path, file_name: &str) -> Result<bool, ObservabilityError> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|source| ObservabilityError::PathIo {
            path: dir.clone(),
            source,
        })? {
            let entry = entry.map_err(|source| ObservabilityError::PathIo {
                path: dir.clone(),
                source,
            })?;
            let path = entry.path();
            if path.file_name().and_then(|name| name.to_str()) == Some(file_name) {
                return Ok(true);
            }
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_owned_vm(root: &Path, vm_id: &str, pid: u32) -> PathBuf {
        let run_dir = root.join(vm_id);
        std::fs::create_dir_all(&run_dir).unwrap();
        std::fs::write(run_dir.join(OWNERSHIP_LOCK), format!("pid={pid}\n")).unwrap();
        run_dir
    }

    #[test]
    fn probe_walks_owned_run_dirs_only() {
        let temp = tempfile::tempdir().unwrap();
        let live = write_owned_vm(temp.path(), "live", std::process::id());
        std::fs::File::create(live.join(crate::DIAGNOSTICS_FILE_NAME)).unwrap();
        std::fs::create_dir_all(live.join("jail")).unwrap();
        std::fs::File::create(live.join("jail").join(FIRECRACKER_API_SOCKET)).unwrap();
        std::fs::File::create(live.join("jail").join(VSOCK_SOCKET)).unwrap();
        std::fs::create_dir_all(temp.path().join("foreign")).unwrap();

        let records = probe(temp.path()).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].vm_id, "live");
        assert_eq!(records[0].health, VmHealth::Healthy);
        assert!(records[0].diagnostics_visible);
    }

    #[test]
    fn health_classification_uses_host_visible_truth_only() {
        assert_eq!(classify(false, true, true), VmHealth::Exited);
        assert_eq!(classify(true, true, true), VmHealth::Healthy);
        assert_eq!(classify(true, true, false), VmHealth::Degraded);
        assert_eq!(classify(true, false, false), VmHealth::Stuck);
    }
}
