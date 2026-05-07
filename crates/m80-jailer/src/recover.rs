//! Inspect a run-dir for prior jail state and decide whether to skip,
//! reap, or `rm -rf`.

use std::io;
use std::path::Path;

use crate::error::JailerError;
use crate::types::{JailerState, Plan, PlanStep, JAILER_PLAN_FILE, JAILER_STATE_FILE};

/// Map an [`std::io::Error`] (or a serde error wrapped in one) to
/// [`JailerError::Io`] for the given `path`. Used as `.map_err(io_err(path))`.
fn io_err(path: std::path::PathBuf) -> impl Fn(io::Error) -> JailerError {
    move |source| JailerError::Io {
        path: path.clone(),
        source,
    }
}

/// Outcome of [`recover_from_run_dir`].
#[derive(Debug, Clone)]
pub enum RecoveryDecision {
    /// A live jailer + firecracker pair was found.
    LiveJail {
        /// PID of the live jailer.
        jailer_pid: u32,
        /// PID of the live firecracker child.
        firecracker_pid: u32,
    },
    /// A stale jail was found; reaping is needed.
    OrphanJail {
        /// Steps the caller should run to clean up.
        reap_steps: Vec<PlanStep>,
    },
    /// No jail was found at this run-dir.
    NoJail,
}

/// Inspect a run-dir for prior jail state. The returned [`RecoveryDecision`]
/// variant tells the caller whether to skip (LiveJail), reap (OrphanJail),
/// or just `rm -rf` (NoJail).
pub fn recover_from_run_dir(run_dir: &Path) -> Result<RecoveryDecision, JailerError> {
    let state_path = run_dir.join(JAILER_STATE_FILE);

    if !state_path.exists() {
        return Ok(RecoveryDecision::NoJail);
    }

    let raw = std::fs::read(&state_path).map_err(io_err(state_path.clone()))?;
    let state: JailerState = serde_json::from_slice(&raw)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        .map_err(io_err(state_path.clone()))?;

    if let (Some(jailer_pid), Some(fc_pid)) = (state.jailer_pid, state.firecracker_pid) {
        if Path::new(&format!("/proc/{jailer_pid}")).exists()
            && Path::new(&format!("/proc/{fc_pid}")).exists()
        {
            return Ok(RecoveryDecision::LiveJail {
                jailer_pid,
                firecracker_pid: fc_pid,
            });
        }
    }

    // Orphan or partial — load plan steps in reverse for reaping.
    let plan_path = run_dir.join(JAILER_PLAN_FILE);
    let reap_steps = if plan_path.exists() {
        let plan_raw = std::fs::read(&plan_path).map_err(io_err(plan_path.clone()))?;
        let plan: Plan = serde_json::from_slice(&plan_raw)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
            .map_err(io_err(plan_path.clone()))?;
        plan.steps.into_iter().rev().collect()
    } else {
        Vec::new()
    };

    Ok(RecoveryDecision::OrphanJail { reap_steps })
}
