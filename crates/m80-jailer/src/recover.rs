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

/// Outcome of [`inspect_run_dir`].
#[derive(Debug, Clone)]
pub enum InspectionDecision {
    /// A live jailer + firecracker pair was found.
    LiveJail {
        /// PID of the live jailer.
        jailer_pid: u32,
        /// PID of the live firecracker child.
        firecracker_pid: u32,
    },
    /// A stale jail was found; reaping is needed.
    OrphanJail {
        /// Opaque plan the caller may hand back to this crate for future reap
        /// support.
        reap_plan: ReapPlan,
    },
    /// No jail was found at this run-dir.
    NoJail,
}

/// Opaque recovery plan for a stale jail.
#[derive(Debug, Clone)]
pub struct ReapPlan {
    steps: Vec<PlanStep>,
}

impl ReapPlan {
    /// Number of materialization steps captured for reverse-order cleanup.
    #[must_use] pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Whether there are no materialization steps available.
    #[must_use] pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

/// Inspect a run-dir for prior jail state. The returned [`InspectionDecision`]
/// variant tells the caller whether to skip (LiveJail), reap (OrphanJail),
/// or just `rm -rf` (NoJail).
pub fn inspect_run_dir(run_dir: &Path) -> Result<InspectionDecision, JailerError> {
    let state_path = run_dir.join(JAILER_STATE_FILE);

    if !state_path.exists() {
        return plan_backed_or_no_jail(run_dir);
    }

    let raw = std::fs::read(&state_path).map_err(io_err(state_path))?;
    let state: JailerState = match serde_json::from_slice(&raw) {
        Ok(state) => state,
        Err(_) => return plan_backed_or_no_jail(run_dir),
    };

    if let (Some(jailer_pid), Some(firecracker_pid)) = (state.jailer_pid, state.firecracker_pid) {
        let jailer_live = jailer_pid == 0 || Path::new(&format!("/proc/{jailer_pid}")).exists();
        let firecracker_live = Path::new(&format!("/proc/{firecracker_pid}")).exists();
        if jailer_live && firecracker_live {
            return Ok(InspectionDecision::LiveJail {
                jailer_pid,
                firecracker_pid,
            });
        }
    }

    // Orphan with parseable state — load plan steps in reverse for reaping.
    Ok(InspectionDecision::OrphanJail {
        reap_plan: load_reap_plan(run_dir)?,
    })
}

fn plan_backed_or_no_jail(run_dir: &Path) -> Result<InspectionDecision, JailerError> {
    let plan_path = run_dir.join(JAILER_PLAN_FILE);
    if plan_path.exists() {
        Ok(InspectionDecision::OrphanJail {
            reap_plan: load_reap_plan(run_dir)?,
        })
    } else {
        Ok(InspectionDecision::NoJail)
    }
}

fn load_reap_plan(run_dir: &Path) -> Result<ReapPlan, JailerError> {
    let plan_path = run_dir.join(JAILER_PLAN_FILE);
    if !plan_path.exists() {
        return Ok(ReapPlan { steps: Vec::new() });
    }

    let plan_raw = std::fs::read(&plan_path).map_err(io_err(plan_path.clone()))?;
    let plan: Plan = serde_json::from_slice(&plan_raw)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        .map_err(io_err(plan_path))?;
    Ok(ReapPlan {
        steps: plan.steps.into_iter().rev().collect(),
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn reap_plan_reverses_plan_steps() {
        let dir = tempfile::tempdir().unwrap();
        let plan = Plan {
            config: crate::types::JailerConfig {
                jailer_bin: PathBuf::from("/usr/bin/jailer"),
                jailer_harden_bin: Some(PathBuf::from("/usr/bin/m80-jailer-harden")),
                firecracker_bin: PathBuf::from("/usr/bin/firecracker"),
                run_dir: dir.path().to_path_buf(),
                uid: 3000,
                gid: 3000,
                bindings: Vec::new(),
                sockets: Vec::new(),
                resource_limits: crate::types::ResourceLimits::default(),
                new_pid_ns: false,
                daemonize: false,
                new_cgroup_ns: false,
                netns_path: None,
                stdio_log: None,
            },
            steps: vec![
                PlanStep::CreateDir {
                    path: PathBuf::from("/jail/root"),
                    mode: 0o700,
                },
                PlanStep::Socket {
                    path: PathBuf::from("/jail/root/firecracker.sock"),
                },
            ],
        };
        std::fs::write(
            dir.path().join(JAILER_PLAN_FILE),
            serde_json::to_vec_pretty(&plan).unwrap(),
        )
        .unwrap();

        let reap_plan = load_reap_plan(dir.path()).unwrap();
        assert_eq!(
            reap_plan.steps,
            plan.steps.into_iter().rev().collect::<Vec<_>>()
        );
    }
}
