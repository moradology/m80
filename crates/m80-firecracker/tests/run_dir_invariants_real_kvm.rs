//! Real-KVM run-dir artifact contract.
//!
//! This test boots a VM and inspects the run-dir immediately after guest
//! readiness, before any user exec request. It is ignored by default because it
//! requires a prepared KVM host with real Firecracker artifacts.

mod common;

use std::collections::BTreeSet;
use std::os::unix::fs::FileTypeExt as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn run_dir_invariants_post_boot() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let firecracker_bin = discovery.firecracker_bin.clone();
    let run_root = discovery.run_root.clone();

    let config = m80_firecracker::BackendConfig::builder(discovery)
        .max_concurrent_vms(1)
        .run_root(run_root)
        .jail_uid(3000)
        .jail_gid(3000)
        .cgroup_mode(m80_firecracker::CgroupMode::Disabled)
        .build();
    let backend = std::sync::Arc::new(m80_firecracker::Backend::new(config).expect("Backend::new"));

    let vm_id = format!("e2e-run-dir-{}", unix_ms_now());
    let sandbox_config = m80_firecracker::SandboxConfig {
        vm_id: Some(vm_id),
        workspace: None,
        network: m80_firecracker::NetworkPolicy::NoEgress,
        vcpu_count: Some(1),
        mem_size_mib: Some(512),
        cpuset_cpus: None,
        cpu_template: None,
        fc_log_level: None,
        drive_cache_type: None,
        boot_args: None,
        overlay_size_bytes: 512 * 1024 * 1024,
        overlay_clone_mode: Default::default(),
        idle_timeout: None,
        max_lifetime: None,
        daemonize: false,
        request_id: Some("req-run-dir-invariants".into()),
        pmem_layers: Vec::new(),
        preallocated_drive_slots: 0,
        one_shot: false,
    };

    let sandbox = backend.admit(sandbox_config).expect("admit");
    let running = sandbox.launch().expect("launch");
    let run_dir = running.run_dir().to_path_buf();
    let _dump_guard = common::RunDirDumpGuard::new(run_dir.clone());

    assert_top_level_run_dir_contract(&run_dir, &firecracker_bin);
    assert_jailer_plan_shape(&run_dir.join(m80_jailer::JAILER_PLAN_FILE));
    assert_jailer_state_shape(&run_dir.join(m80_jailer::JAILER_STATE_FILE));
    assert_boot_identity_shape(&m80_firecracker::boot_identity_path(&run_dir));
    assert_diagnostics_shape(&run_dir.join(m80_observability::DIAGNOSTICS_FILE_NAME));
    assert_regular_file(&m80_firecracker::console_log_path(&run_dir));
    assert_regular_file(&m80_firecracker::rootfs_overlay_path(&run_dir));
    assert_unix_socket(&m80_firecracker::firecracker_api_socket_path(
        &run_dir,
        &firecracker_bin,
    ));
    assert_unix_socket(&m80_firecracker::vsock_socket_path(
        &run_dir,
        &firecracker_bin,
    ));

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
    assert!(
        !run_dir.exists(),
        "delete must remove the run-dir and its {} file: {}",
        m80_firecracker::OWNERSHIP_LOCK,
        run_dir.display()
    );
}

fn assert_top_level_run_dir_contract(run_dir: &Path, firecracker_bin: &Path) {
    let jailer_root_parent = firecracker_bin
        .file_name()
        .expect("firecracker binary basename")
        .to_string_lossy()
        .into_owned();
    let expected = BTreeSet::from([
        m80_firecracker::BOOT_IDENTITY_FILE.to_owned(),
        m80_firecracker::CONSOLE_LOG.to_owned(),
        m80_firecracker::OWNERSHIP_LOCK.to_owned(),
        m80_firecracker::ROOTFS_OVERLAY_IMAGE.to_owned(),
        m80_jailer::JAILER_PLAN_FILE.to_owned(),
        m80_jailer::JAILER_STATE_FILE.to_owned(),
        m80_observability::DIAGNOSTICS_FILE_NAME.to_owned(),
        "snapshot".to_owned(),
        jailer_root_parent,
    ]);
    assert_eq!(top_level_names(run_dir), expected);
    assert!(
        run_dir.join("snapshot").is_dir(),
        "snapshot staging path must be a directory"
    );
}

fn top_level_names(run_dir: &Path) -> BTreeSet<String> {
    std::fs::read_dir(run_dir)
        .unwrap_or_else(|e| panic!("read run-dir {}: {e}", run_dir.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|e| panic!("read run-dir entry in {}: {e}", run_dir.display()))
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

fn assert_jailer_plan_shape(path: &Path) {
    let json = read_json(path);
    assert!(json.get("config").is_some(), "missing config in {path:?}");
    assert!(
        json.get("steps").and_then(Value::as_array).is_some(),
        "missing steps array in {path:?}: {json}"
    );
}

fn assert_jailer_state_shape(path: &Path) {
    let json = read_json(path);
    let firecracker_pid = json
        .get("firecracker_pid")
        .and_then(Value::as_u64)
        .expect("firecracker_pid");
    let jailer_pid = json
        .get("jailer_pid")
        .and_then(Value::as_u64)
        .expect("jailer_pid");
    assert!(firecracker_pid > 0, "firecracker_pid must be nonzero");
    assert!(
        PathBuf::from(format!("/proc/{firecracker_pid}")).exists(),
        "firecracker pid {firecracker_pid} must be live"
    );
    if jailer_pid != 0 {
        assert!(
            PathBuf::from(format!("/proc/{jailer_pid}")).exists(),
            "nonzero jailer pid {jailer_pid} must be live"
        );
    }
}

fn assert_boot_identity_shape(path: &Path) {
    let json = read_json(path);
    assert_eq!(json["schema_version"], 1);
    for key in [
        "kernel",
        "kernel_sha256",
        "rootfs",
        "rootfs_sha256",
        "manifest_sha256",
        "expected_firecracker_version",
        "guest_port",
        "ready_marker",
    ] {
        assert!(json.get(key).is_some(), "missing {key} in {path:?}: {json}");
    }
}

fn assert_diagnostics_shape(path: &Path) {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read diagnostics {}: {e}", path.display()));
    let mut events = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let json: Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("diagnostics line {} invalid JSON: {e}: {line}", idx + 1));
        assert_eq!(json["schema_version"], 2);
        assert!(
            json.get("timestamp_unix_ms")
                .and_then(Value::as_u64)
                .is_some(),
            "diagnostics line {} missing timestamp: {json}",
            idx + 1
        );
        assert!(
            json.get("phase").and_then(Value::as_str).is_some(),
            "diagnostics line {} missing phase: {json}",
            idx + 1
        );
        assert!(
            json.get("message").and_then(Value::as_str).is_some(),
            "diagnostics line {} missing message: {json}",
            idx + 1
        );
        events.push(json);
    }
    assert!(!events.is_empty(), "diagnostics must contain events");
    assert!(
        events
            .iter()
            .any(|event| event.get("message").and_then(Value::as_str) == Some("guestd ready")),
        "diagnostics must include guestd readiness event: {text}"
    );
}

fn assert_regular_file(path: &Path) {
    let meta =
        std::fs::metadata(path).unwrap_or_else(|e| panic!("metadata {}: {e}", path.display()));
    assert!(meta.is_file(), "{} must be a regular file", path.display());
}

fn assert_unix_socket(path: &Path) {
    let meta = std::fs::symlink_metadata(path)
        .unwrap_or_else(|e| panic!("metadata {}: {e}", path.display()));
    assert!(
        meta.file_type().is_socket(),
        "{} must be a Unix domain socket",
        path.display()
    );
}

fn read_json(path: &Path) -> Value {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}: {text}", path.display()))
}

fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before Unix epoch")
        .as_millis() as u64
}
