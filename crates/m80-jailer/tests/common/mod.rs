//! Shared test fixtures for m80-jailer integration tests.

use std::path::{Path, PathBuf};

use m80_jailer::JailerConfig;
use m80_jailer::{BindMode, Plan, PlanStep};

/// Minimal `JailerConfig` (uid/gid 3000, no bindings, no sockets). Tests
/// extend this with their own bindings/sockets as needed.
pub fn minimal_config(run_dir: &Path) -> JailerConfig {
    JailerConfig {
        jailer_bin: PathBuf::from("/usr/bin/jailer"),
        jailer_harden_bin: Some(PathBuf::from("/usr/bin/m80-jailer-harden")),
        firecracker_bin: PathBuf::from("/usr/bin/firecracker"),
        run_dir: run_dir.to_path_buf(),
        uid: 3000,
        gid: 3000,
        bindings: Vec::new(),
        sockets: Vec::new(),
        resource_limits: m80_jailer::ResourceLimits::default(),
        new_pid_ns: false,
        daemonize: false,
        netns_path: None,
        stdio_log: None,
    }
}

/// Extract all `Bind` steps from a plan as `(source, dest, mode)` tuples.
#[allow(dead_code)]
pub fn bind_steps(plan: &Plan) -> Vec<(&Path, &Path, BindMode)> {
    plan.steps
        .iter()
        .filter_map(|step| {
            if let PlanStep::Bind { source, dest, mode } = step {
                Some((source.as_path(), dest.as_path(), *mode))
            } else {
                None
            }
        })
        .collect()
}

/// Extract all `Socket` paths from a plan.
#[allow(dead_code)]
pub fn socket_steps(plan: &Plan) -> Vec<&Path> {
    plan.steps
        .iter()
        .filter_map(|step| {
            if let PlanStep::Socket { path } = step {
                Some(path.as_path())
            } else {
                None
            }
        })
        .collect()
}
