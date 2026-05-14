//! Real-KVM snapshot concurrency coverage.
//!
//! Ignored by default because it needs a KVM-capable host, snapshot-capable
//! Firecracker artifacts, and enough headroom for concurrent VMs.

mod common;

use std::path::Path;
use std::sync::{Arc, Barrier};
use std::time::{SystemTime, UNIX_EPOCH};

use common::RunDirDumpGuard;
use m80_firecracker::{Backend, BackendConfig, CgroupMode, SandboxConfig, SnapshotPaths};
use m80_proto::{ExecRequest, ExecStatus};

#[test]
#[ignore = "requires KVM host with real Firecracker binary and snapshot support"]
fn snapshot_concurrent_capture_and_restore() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let initial_snap_dir = discovery
        .run_root
        .join(format!("snap-concurrency-initial-{}", unique_suffix()));
    let recapture_snap_dir = discovery
        .run_root
        .join(format!("snap-concurrency-recapture-{}", unique_suffix()));
    let initial_paths = snapshot_paths(&initial_snap_dir);
    let recapture_paths = snapshot_paths(&recapture_snap_dir);
    let backend = Arc::new(Backend::new(make_backend_config(discovery.clone())).unwrap());

    let golden = backend
        .admit(sandbox_config("snap-concurrency-golden"))
        .expect("admit golden");
    let mut golden = golden.launch().expect("launch golden");
    let _golden_dump = RunDirDumpGuard::new(golden.run_dir().to_path_buf());
    assert_exec_ok(&mut golden, "echo initial > /tmp/snapshot-race");
    golden
        .capture(initial_paths.clone())
        .expect("initial capture");
    golden
        .stop()
        .expect("stop golden")
        .delete()
        .expect("delete golden");

    let recapture = backend
        .admit(sandbox_config("snap-concurrency-recapture"))
        .expect("admit recapture");
    let mut recapture = recapture.launch().expect("launch recapture");
    let _recapture_dump = RunDirDumpGuard::new(recapture.run_dir().to_path_buf());
    assert_exec_ok(&mut recapture, "echo recaptured > /tmp/snapshot-race");

    let barrier = Arc::new(Barrier::new(2));
    let restore_backend = Arc::clone(&backend);
    let restore_discovery = discovery.clone();
    let restore_paths = initial_paths.clone();
    let restore_barrier = Arc::clone(&barrier);
    let restore = std::thread::spawn(move || {
        restore_barrier.wait();
        let sandbox = restore_backend
            .admit(sandbox_config("snap-concurrency-restoring"))
            .expect("admit restoring");
        let mut restored = sandbox
            .launch_from_snapshot(restore_paths, &restore_discovery)
            .expect("launch restoring");
        let _restore_dump = RunDirDumpGuard::new(restored.run_dir().to_path_buf());
        assert_exec_ok(&mut restored, "cat /tmp/snapshot-race >/dev/null");
        restored
            .stop()
            .expect("stop restoring")
            .delete()
            .expect("delete restoring");
    });

    // Snapshot files are caller-owned artifacts, not an internally locked
    // database. This test covers concurrent capture and restore operations,
    // not simultaneous read/write mutation of the same snapshot pair.
    let capture_paths = recapture_paths.clone();
    let capture_barrier = Arc::clone(&barrier);
    let capture = std::thread::spawn(move || {
        capture_barrier.wait();
        recapture.capture(capture_paths).expect("recapture");
        recapture
            .stop()
            .expect("stop recapture")
            .delete()
            .expect("delete recapture");
    });

    restore.join().expect("restore thread");
    capture.join().expect("capture thread");

    assert_snapshot_files_nonempty(&initial_paths);
    assert_snapshot_files_nonempty(&recapture_paths);

    let final_backend = Arc::new(Backend::new(make_backend_config(discovery.clone())).unwrap());
    let final_sandbox = final_backend
        .admit(sandbox_config("snap-concurrency-final"))
        .expect("admit final restore");
    let mut final_restore = final_sandbox
        .launch_from_snapshot(recapture_paths, &discovery)
        .expect("final restore after concurrent capture");
    let _final_dump = RunDirDumpGuard::new(final_restore.run_dir().to_path_buf());
    assert_exec_ok(&mut final_restore, "cat /tmp/snapshot-race >/dev/null");
    final_restore
        .stop()
        .expect("stop final restore")
        .delete()
        .expect("delete final restore");

    let _ = std::fs::remove_dir_all(initial_snap_dir);
    let _ = std::fs::remove_dir_all(recapture_snap_dir);
}

fn make_backend_config(discovery: m80_preflight::Discovery) -> BackendConfig {
    let run_root = discovery.run_root.clone();
    BackendConfig::builder(discovery)
        .max_concurrent_vms(4)
        .run_root(run_root)
        .jail_uid(3000)
        .jail_gid(3000)
        .cgroup_mode(CgroupMode::Disabled)
        .build()
}

fn sandbox_config(vm_id: impl Into<String>) -> SandboxConfig {
    SandboxConfig {
        vcpu_count: Some(m80_firecracker::FIRST_LINE_VCPU_COUNT),
        mem_size_mib: Some(m80_firecracker::FIRST_LINE_MEM_SIZE_MIB),
        cpuset_cpus: None,
        cpu_template: None,
        ..common::sandbox_config_with_id(vm_id)
    }
}

fn snapshot_paths(dir: &Path) -> SnapshotPaths {
    std::fs::create_dir_all(dir).expect("create snapshot dir");
    SnapshotPaths {
        vm_state: dir.join("vm.snap"),
        mem: dir.join("mem.snap"),
    }
}

fn assert_snapshot_files_nonempty(paths: &SnapshotPaths) {
    for path in [&paths.vm_state, &paths.mem] {
        let len = std::fs::metadata(path)
            .unwrap_or_else(|e| panic!("metadata {}: {e}", path.display()))
            .len();
        assert!(
            len > 0,
            "snapshot file must not be empty: {}",
            path.display()
        );
    }
}

fn assert_exec_ok(running: &mut m80_firecracker::RunningSandbox, script: &str) {
    let response = running
        .exec(ExecRequest {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        })
        .unwrap_or_else(|e| panic!("exec {script:?}: {e}"));
    assert_eq!(response.status, ExecStatus::Completed);
    assert_eq!(
        response.exit_code,
        Some(0),
        "exec {script:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&response.stdout),
        String::from_utf8_lossy(&response.stderr)
    );
}

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos()
}
