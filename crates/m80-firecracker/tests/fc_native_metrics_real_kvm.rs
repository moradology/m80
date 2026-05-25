//! Real-KVM check that Firecracker's native metrics file is configured.

mod common;

use std::path::{Path, PathBuf};

use m80_firecracker::SnapshotPaths;

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn fc_native_metrics_file_exists_after_launch() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let backend = common::make_backend(discovery);
    let vm_id = common::unique_vm_id("fc-metrics");
    let sandbox = backend
        .admit(common::sandbox_config_with_id(vm_id))
        .expect("admit");
    let running = sandbox.launch().expect("launch");
    let run_dir = running.run_dir().to_path_buf();
    let _dump = common::RunDirDumpGuard::new(run_dir);

    let metrics_path = running.fc_metrics_path();
    assert!(
        metrics_path.is_file(),
        "Firecracker native metrics file missing at {}",
        metrics_path.display()
    );

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary and snapshot support"]
fn fc_native_metrics_file_exists_after_snapshot_restore() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let snap_dir = warm_snapshot_dir(
        &discovery,
        &format!(
            "fc-native-metrics-restore-{:04x}",
            common::unique_suffix() % 0x10000
        ),
    );
    let paths = snapshot_paths(&snap_dir);

    let backend = common::make_backend(discovery.clone());
    let golden = backend
        .admit(common::sandbox_config_with_id(common::unique_vm_id(
            "fc-metrics-golden",
        )))
        .expect("admit golden");
    let mut running = golden.launch().expect("launch golden");
    let _dump = common::RunDirDumpGuard::new(running.run_dir().to_path_buf());

    running.capture(paths.clone()).expect("capture snapshot");
    let stopped = running.stop().expect("stop golden");
    stopped.delete().expect("delete golden");

    let restore_backend = common::make_backend(discovery.clone());
    let restore_sandbox = restore_backend
        .admit(common::sandbox_config_with_id(common::unique_vm_id(
            "fc-metrics-restored",
        )))
        .expect("admit restored");
    let restored = restore_sandbox
        .launch_from_snapshot(paths, &discovery)
        .expect("launch restored");
    let _dump = common::RunDirDumpGuard::new(restored.run_dir().to_path_buf());

    let metrics_path = restored.fc_metrics_path();
    assert!(
        metrics_path.is_file(),
        "Firecracker native metrics file missing after restore at {}",
        metrics_path.display()
    );

    let stopped = restored.stop().expect("stop restored");
    stopped.delete().expect("delete restored");
    let _ = std::fs::remove_dir_all(&snap_dir);
}

fn snapshot_paths(dir: &Path) -> SnapshotPaths {
    std::fs::create_dir_all(dir).expect("create snapshot dir");
    SnapshotPaths {
        vm_state: dir.join("vm.snap"),
        mem: dir.join("mem.snap"),
    }
}

fn warm_snapshot_dir(discovery: &m80_preflight::Discovery, name: &str) -> PathBuf {
    discovery.run_root.join("warm").join(name)
}
