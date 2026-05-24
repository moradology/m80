//! Shared test fixtures for m80-jailer integration tests.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};

use m80_jailer::JailerConfig;
use m80_jailer::{BindMode, Plan};

/// Minimal `JailerConfig` (uid/gid 3000, no bindings, no sockets). Tests
/// extend this with their own bindings/sockets as needed.
pub(crate) fn minimal_config(run_dir: &Path) -> JailerConfig {
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
        new_net_ns: false,
        daemonize: false,
        new_cgroup_ns: false,
        cgroup_version: None,
        netns_path: None,
        seccomp_filter_path: None,
        stdio_log: None,
    }
}

pub(crate) fn steps(plan: &Plan) -> Vec<serde_json::Value> {
    serde_json::to_value(plan).unwrap()["steps"]
        .as_array()
        .expect("steps array")
        .clone()
}

/// Extract all `Bind` steps from a plan as `(source, dest, mode)` tuples.
#[allow(dead_code)]
pub(crate) fn bind_steps(plan: &Plan) -> Vec<(PathBuf, PathBuf, BindMode)> {
    steps(plan)
        .into_iter()
        .filter_map(|step| {
            if step["kind"] == "bind" {
                let source = PathBuf::from(step["source"].as_str().expect("bind source"));
                let dest = PathBuf::from(step["dest"].as_str().expect("bind dest"));
                let mode: BindMode =
                    serde_json::from_value(step["mode"].clone()).expect("bind mode");
                Some((source, dest, mode))
            } else {
                None
            }
        })
        .collect()
}

/// Extract all `Socket` paths from a plan.
#[allow(dead_code)]
pub(crate) fn socket_steps(plan: &Plan) -> Vec<PathBuf> {
    steps(plan)
        .into_iter()
        .filter_map(|step| {
            if step["kind"] == "socket" {
                Some(PathBuf::from(step["path"].as_str().expect("socket path")))
            } else {
                None
            }
        })
        .collect()
}
