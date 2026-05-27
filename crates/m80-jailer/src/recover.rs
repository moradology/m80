//! Inspect a run-dir for prior jail state and decide whether to skip,
//! reap, or `rm -rf`.

use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::Command;

use tracing::warn;

use crate::error::JailerError;
use crate::types::{
    JailerState, Plan, PlanStep, JAILER_PLAN_FILE, JAILER_STATE_FILE, JAILER_SYSTEMD_UNIT_FILE,
};

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
    /// A live transient systemd unit was found before pid state was persisted.
    LiveSystemdUnit {
        /// Transient unit name recorded by the launch path.
        unit_name: String,
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
    #[must_use]
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Whether there are no materialization steps available.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

/// Inspect a run-dir for prior jail state. The returned [`InspectionDecision`]
/// variant tells the caller whether to skip (LiveJail), reap (OrphanJail),
/// or just `rm -rf` (NoJail).
pub fn inspect_run_dir(run_dir: &Path) -> Result<InspectionDecision, JailerError> {
    inspect_run_dir_with_systemd_probe(run_dir, systemd_unit_state)
}

fn inspect_run_dir_with_systemd_probe<F>(
    run_dir: &Path,
    mut systemd_unit_state: F,
) -> Result<InspectionDecision, JailerError>
where
    F: FnMut(&str) -> Result<SystemdUnitState, JailerError>,
{
    let state_path = run_dir.join(JAILER_STATE_FILE);

    if !state_path.exists() {
        return plan_backed_or_no_jail(run_dir, &mut systemd_unit_state);
    }

    let raw = std::fs::read(&state_path).map_err(io_err(state_path))?;
    let state: JailerState = match serde_json::from_slice(&raw) {
        Ok(state) => state,
        Err(e) => {
            warn!(path = %run_dir.display(), error = %e, "jailer-state.json is corrupt; treating as orphan");
            return plan_backed_or_no_jail(run_dir, &mut systemd_unit_state);
        }
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

fn plan_backed_or_no_jail<F>(
    run_dir: &Path,
    systemd_unit_state: &mut F,
) -> Result<InspectionDecision, JailerError>
where
    F: FnMut(&str) -> Result<SystemdUnitState, JailerError>,
{
    let plan_path = run_dir.join(JAILER_PLAN_FILE);
    if plan_path.exists() {
        if let Some(decision) = live_systemd_unit(run_dir, systemd_unit_state)? {
            return Ok(decision);
        }
        Ok(InspectionDecision::OrphanJail {
            reap_plan: load_reap_plan(run_dir)?,
        })
    } else {
        Ok(InspectionDecision::NoJail)
    }
}

fn live_systemd_unit<F>(
    run_dir: &Path,
    systemd_unit_state: &mut F,
) -> Result<Option<InspectionDecision>, JailerError>
where
    F: FnMut(&str) -> Result<SystemdUnitState, JailerError>,
{
    let unit_path = run_dir.join(JAILER_SYSTEMD_UNIT_FILE);
    if !unit_path.exists() {
        return Ok(None);
    }

    let unit_name = read_systemd_unit_marker(&unit_path)?;
    match systemd_unit_state(&unit_name)? {
        SystemdUnitState::Live => Ok(Some(InspectionDecision::LiveSystemdUnit { unit_name })),
        SystemdUnitState::Inactive => Ok(None),
    }
}

fn read_systemd_unit_marker(path: &Path) -> Result<String, JailerError> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
        .open(path)
        .map_err(io_err(path.to_path_buf()))?;
    let mut raw = String::new();
    std::io::Read::read_to_string(&mut file, &mut raw).map_err(io_err(path.to_path_buf()))?;
    let unit_name = raw.trim();
    if is_m80_systemd_unit_name(unit_name) {
        Ok(unit_name.to_owned())
    } else {
        Err(JailerError::Io {
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid m80 systemd unit marker: {unit_name:?}"),
            ),
        })
    }
}

fn is_m80_systemd_unit_name(unit_name: &str) -> bool {
    unit_name.starts_with("m80-vm-")
        && unit_name.len() <= 128
        && unit_name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SystemdUnitState {
    Live,
    Inactive,
}

fn systemd_unit_state(unit_name: &str) -> Result<SystemdUnitState, JailerError> {
    let output = Command::new("systemctl")
        .arg("show")
        .arg("--property=ActiveState")
        .arg("--value")
        .arg(unit_name)
        .output()
        .map_err(|source| JailerError::Io {
            path: "systemctl".into(),
            source,
        })?;
    if !output.status.success() {
        return Err(systemd_probe_error(
            unit_name,
            "systemctl show ActiveState failed",
            command_output_detail(&output),
        ));
    }

    parse_systemd_active_state(unit_name, String::from_utf8_lossy(&output.stdout).trim())
}

fn parse_systemd_active_state(
    unit_name: &str,
    active_state: &str,
) -> Result<SystemdUnitState, JailerError> {
    match active_state {
        "active" | "activating" | "reloading" | "deactivating" => Ok(SystemdUnitState::Live),
        "inactive" | "failed" => Ok(SystemdUnitState::Inactive),
        other => Err(systemd_probe_error(
            unit_name,
            "systemctl returned unsupported ActiveState",
            other.to_owned(),
        )),
    }
}

fn systemd_probe_error(unit_name: &str, message: &str, detail: String) -> JailerError {
    JailerError::Io {
        path: "systemctl".into(),
        source: io::Error::new(
            io::ErrorKind::Other,
            format!("{message} for {unit_name}: {detail}"),
        ),
    }
}

fn command_output_detail(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!("stdout={stdout}"),
        (true, false) => format!("stderr={stderr}"),
        (false, false) => format!("stdout={stdout}; stderr={stderr}"),
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
    #![allow(clippy::unwrap_used)]

    use std::path::PathBuf;

    use super::*;

    #[test]
    fn reap_plan_reverses_plan_steps() {
        let dir = tempfile::tempdir().unwrap();
        let plan = Plan {
            schema_version: 1,
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
                new_net_ns: false,
                daemonize: false,
                new_cgroup_ns: false,
                cgroup_version: None,
                netns_path: None,
                seccomp_filter_path: None,
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

    #[test]
    fn plan_with_active_systemd_unit_marker_returns_live_systemd_unit() {
        let dir = tempfile::tempdir().unwrap();
        write_test_plan(dir.path());
        std::fs::write(
            dir.path().join(JAILER_SYSTEMD_UNIT_FILE),
            b"m80-vm-aaaaaaaaaaaaaaaaaaaaaaaa\n",
        )
        .unwrap();

        let decision = inspect_run_dir_with_systemd_probe(dir.path(), |unit| {
            assert_eq!(unit, "m80-vm-aaaaaaaaaaaaaaaaaaaaaaaa");
            Ok(SystemdUnitState::Live)
        })
        .unwrap();

        assert!(matches!(
            decision,
            InspectionDecision::LiveSystemdUnit { unit_name }
                if unit_name == "m80-vm-aaaaaaaaaaaaaaaaaaaaaaaa"
        ));
    }

    #[test]
    fn plan_with_inactive_systemd_unit_marker_returns_orphan() {
        let dir = tempfile::tempdir().unwrap();
        write_test_plan(dir.path());
        std::fs::write(
            dir.path().join(JAILER_SYSTEMD_UNIT_FILE),
            b"m80-vm-bbbbbbbbbbbbbbbbbbbbbbbb\n",
        )
        .unwrap();

        let decision =
            inspect_run_dir_with_systemd_probe(dir.path(), |_| Ok(SystemdUnitState::Inactive))
                .unwrap();

        assert!(matches!(decision, InspectionDecision::OrphanJail { .. }));
    }

    #[test]
    fn plan_with_systemd_probe_error_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        write_test_plan(dir.path());
        std::fs::write(
            dir.path().join(JAILER_SYSTEMD_UNIT_FILE),
            b"m80-vm-cccccccccccccccccccccccc\n",
        )
        .unwrap();

        let err = inspect_run_dir_with_systemd_probe(dir.path(), |_| {
            Err(JailerError::Io {
                path: "systemctl".into(),
                source: io::Error::new(io::ErrorKind::Other, "dbus unavailable"),
            })
        })
        .unwrap_err();

        assert!(
            err.to_string().contains("dbus unavailable"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn invalid_systemd_unit_marker_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        write_test_plan(dir.path());
        std::fs::write(
            dir.path().join(JAILER_SYSTEMD_UNIT_FILE),
            b"not an m80 unit\n",
        )
        .unwrap();

        let err =
            inspect_run_dir_with_systemd_probe(dir.path(), |_| Ok(SystemdUnitState::Inactive))
                .unwrap_err();

        assert!(
            err.to_string().contains("invalid m80 systemd unit marker"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn activating_systemd_unit_state_counts_as_live() {
        let state = parse_systemd_active_state("m80-vm-test", "activating").unwrap();

        assert_eq!(state, SystemdUnitState::Live);
    }

    #[test]
    fn inactive_systemd_unit_state_counts_as_not_live() {
        let state = parse_systemd_active_state("m80-vm-test", "inactive").unwrap();

        assert_eq!(state, SystemdUnitState::Inactive);
    }

    fn write_test_plan(run_dir: &Path) {
        let plan = Plan {
            schema_version: 1,
            config: crate::types::JailerConfig {
                jailer_bin: PathBuf::from("/usr/bin/jailer"),
                jailer_harden_bin: Some(PathBuf::from("/usr/bin/m80-jailer-harden")),
                firecracker_bin: PathBuf::from("/usr/bin/firecracker"),
                run_dir: run_dir.to_path_buf(),
                uid: 3000,
                gid: 3000,
                bindings: Vec::new(),
                sockets: Vec::new(),
                resource_limits: crate::types::ResourceLimits::default(),
                new_pid_ns: false,
                new_net_ns: false,
                daemonize: false,
                new_cgroup_ns: false,
                cgroup_version: None,
                netns_path: None,
                seccomp_filter_path: None,
                stdio_log: None,
            },
            steps: vec![PlanStep::CreateDir {
                path: PathBuf::from("/jail/root"),
                mode: 0o700,
            }],
        };
        std::fs::write(
            run_dir.join(JAILER_PLAN_FILE),
            serde_json::to_vec_pretty(&plan).unwrap(),
        )
        .unwrap();
    }
}
