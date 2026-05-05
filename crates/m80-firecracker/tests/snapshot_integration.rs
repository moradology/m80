//! Snapshot integration tests: capture → restore round-trip, readiness after
//! restore, and clear failure on missing snapshot files.
//!
//! All three tests that exercise real KVM are `#[ignore]`d by default.
//! Run on a KVM-capable host with m80 artifacts:
//!
//! ```
//! sudo cargo test -p m80-firecracker -- --ignored snapshot_integration
//! ```
//!
//! See `end_to_end_real_kvm.rs` for environment-variable conventions.

mod common;
use common::RunDirDumpGuard;

use std::path::PathBuf;

use m80_firecracker::{
    Backend, BackendConfig, CgroupMode, NetworkPolicy, SandboxConfig, SnapshotPaths,
};

/// Build a `BackendConfig` from `m80_preflight::run()`.
fn make_backend_config(discovery: m80_preflight::Discovery) -> BackendConfig {
    let run_root = discovery.run_root.clone();
    BackendConfig {
        discovery,
        max_concurrent_vms: 1,
        run_root,
        jail_uid: 3000,
        jail_gid: 3000,
        cgroup_mode: CgroupMode::Disabled,
    }
}

/// Minimal sandbox config shared across tests.
fn sandbox_config(vm_id: impl Into<String>) -> SandboxConfig {
    SandboxConfig {
        vm_id: Some(vm_id.into()),
        workspace: None,
        network: NetworkPolicy::NoEgress,
        vcpu_count: Some(1),
        mem_size_mib: Some(512),
        boot_args: None,
        overlay_size_bytes: 512 * 1024 * 1024,
        idle_timeout: None,
    }
}

/// Create a `SnapshotPaths` in the given directory, creating the dir if absent.
fn snapshot_paths(dir: &std::path::Path) -> SnapshotPaths {
    std::fs::create_dir_all(dir).expect("create snapshot dir");
    SnapshotPaths {
        vm_state: dir.join("vm.snap"),
        mem: dir.join("mem.snap"),
    }
}

// ---------------------------------------------------------------------------
// Test 1: full round-trip — write a marker file before capture, read it back
//         after restore.
// ---------------------------------------------------------------------------

/// Launch a VM, write a marker file via exec, capture, stop, restore, exec
/// again to read the marker. Asserts the marker is visible in the restored VM.
///
/// This verifies that the snapshot preserves in-VM state (the guest memory
/// and disk state) and that exec dispatch works post-restore.
#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn capture_then_restore_round_trip() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let snap_dir = discovery.run_root.join("snap-round-trip");

    let config = make_backend_config(discovery.clone());
    let backend = std::sync::Arc::new(Backend::new(config).expect("Backend::new"));

    // --- Boot golden VM ---
    let golden = backend
        .admit(sandbox_config("snap-golden"))
        .expect("admit golden");
    let mut running = golden.launch().expect("launch golden");
    let _dump = RunDirDumpGuard::new(running.run_dir().to_path_buf());

    // Write a marker file.
    let write_resp = running
        .exec(m80_proto::ExecRequest {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "echo m80-marker > /tmp/marker.txt".into()],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
        })
        .expect("exec: write marker");
    assert_eq!(write_resp.exit_code, Some(0), "marker write must succeed");

    // Capture.
    let paths = snapshot_paths(&snap_dir);
    running.capture(paths.clone()).expect("capture");

    // Stop (VM is Paused after capture; stop kills the process).
    let stopped = running.stop().expect("stop golden");
    stopped.delete().expect("delete golden run-dir");

    // --- Restore VM ---
    let restore_backend =
        std::sync::Arc::new(Backend::new(make_backend_config(discovery.clone())).expect("Backend::new restore"));
    let restore_sandbox = restore_backend
        .admit(sandbox_config("snap-restored"))
        .expect("admit restore");

    let mut restored = restore_sandbox
        .launch_from_snapshot(paths, &discovery)
        .expect("launch_from_snapshot");
    let _dump2 = RunDirDumpGuard::new(restored.run_dir().to_path_buf());

    // Read the marker — it must be visible.
    let read_resp = restored
        .exec(m80_proto::ExecRequest {
            program: "/bin/cat".into(),
            args: vec!["/tmp/marker.txt".into()],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
        })
        .expect("exec: read marker");
    assert_eq!(read_resp.exit_code, Some(0), "marker read must succeed");
    let stdout = String::from_utf8_lossy(&read_resp.stdout);
    assert!(
        stdout.trim() == "m80-marker",
        "expected 'm80-marker', got {stdout:?}"
    );

    let stopped2 = restored.stop().expect("stop restored");
    stopped2.delete().expect("delete restored run-dir");

    // Clean up snapshot directory.
    let _ = std::fs::remove_dir_all(&snap_dir);
}

// ---------------------------------------------------------------------------
// Test 2: exec works after restore even without a prior exec in the golden VM.
//         Verifies the vsock probe path (no "warm-up" exec helps the probe).
// ---------------------------------------------------------------------------

/// Launch a VM, capture immediately (no exec in the golden VM), stop, restore,
/// then exec a simple command. Asserts exec succeeds, proving that the vsock
/// probe correctly waits for the exec channel to become live.
#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn restore_executes_after_idle() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let snap_dir = discovery.run_root.join("snap-idle");

    let config = make_backend_config(discovery.clone());
    let backend = std::sync::Arc::new(Backend::new(config).expect("Backend::new"));

    // Boot golden VM but do NOT exec anything before capture.
    let golden = backend
        .admit(sandbox_config("snap-idle-golden"))
        .expect("admit golden");
    let running = golden.launch().expect("launch golden");
    let _dump = RunDirDumpGuard::new(running.run_dir().to_path_buf());

    let paths = snapshot_paths(&snap_dir);
    running.capture(paths.clone()).expect("capture idle");

    let stopped = running.stop().expect("stop idle golden");
    stopped.delete().expect("delete idle golden run-dir");

    // Restore.
    let restore_backend =
        std::sync::Arc::new(Backend::new(make_backend_config(discovery.clone())).expect("Backend::new restore"));
    let restore_sandbox = restore_backend
        .admit(sandbox_config("snap-idle-restored"))
        .expect("admit restore");

    let mut restored = restore_sandbox
        .launch_from_snapshot(paths, &discovery)
        .expect("launch_from_snapshot idle");
    let _dump2 = RunDirDumpGuard::new(restored.run_dir().to_path_buf());

    // Exec a trivial command — asserts the exec channel is live.
    let resp = restored
        .exec(m80_proto::ExecRequest {
            program: "/bin/echo".into(),
            args: vec!["restored".into()],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
        })
        .expect("exec after idle restore");
    assert_eq!(resp.exit_code, Some(0));
    let stdout = String::from_utf8_lossy(&resp.stdout);
    assert!(stdout.trim() == "restored", "expected 'restored', got {stdout:?}");

    let stopped2 = restored.stop().expect("stop idle restored");
    stopped2.delete().expect("delete idle restored run-dir");

    let _ = std::fs::remove_dir_all(&snap_dir);
}

// ---------------------------------------------------------------------------
// Test 3: missing snapshot files produce a clearly-classified error (not a
//         generic IO error swallow).
//
// This test does NOT require KVM — it calls launch_from_snapshot with a
// Firecracker binary path that will fail at spawn time; but the snapshot
// path non-existence check comes before the load REST call, so the returned
// error should be a `Snapshot` or `Jailer`/`Client` variant — never a silent
// `Ok`. The important property is: the error is not swallowed.
//
// We mark it #[ignore] as well since it still needs the jailer/preflight
// environment (to get past phases 1-4), but add a non-ignored version that
// at least compiles and tests the error variant's classification without KVM.
// ---------------------------------------------------------------------------

/// Call `launch_from_snapshot` with snapshot paths that do not exist.
/// Assert the error is a typed `FcError`, not a panic, and is surfaced
/// (not silently recovered). The test needs KVM to get past the jailer phase.
#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn restore_with_missing_snapshot_fails_clearly() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");

    let config = make_backend_config(discovery.clone());
    let backend = std::sync::Arc::new(Backend::new(config).expect("Backend::new"));

    let sandbox = backend
        .admit(sandbox_config("snap-missing"))
        .expect("admit");

    // Point at paths that do not exist.
    let missing_paths = SnapshotPaths {
        vm_state: PathBuf::from("/tmp/m80-nonexistent/vm.snap"),
        mem: PathBuf::from("/tmp/m80-nonexistent/mem.snap"),
    };

    let result = sandbox.launch_from_snapshot(missing_paths, &discovery);

    // Must fail — not silently succeed.
    assert!(
        result.is_err(),
        "launch_from_snapshot with missing snapshot must return an error"
    );

    // The error must be a typed FcError (Snapshot or Client), not a generic
    // IO error that swallows the context. Formatting it must include a hint
    // about the failure origin.
    let err = result.unwrap_err();
    let display = err.to_string();
    assert!(
        display.contains("snapshot") || display.contains("client") || display.contains("i/o"),
        "error must be clearly classified, got: {display}"
    );
}

/// Non-KVM variant: verify that `SnapshotPaths` missing-file errors surface
/// as typed errors via `m80_snapshot::restore` directly (no KVM required).
///
/// This test covers the classification property without a live Firecracker.
#[test]
fn missing_snapshot_error_is_classified_without_kvm() {
    use m80_snapshot::{restore, RestoreRequest, SnapshotPaths};

    // Use a non-existent Firecracker socket — the vsock.sock removal step will
    // succeed (ENOENT is silenced) and then the Client::new call will fail with
    // a typed ClientError, not a generic IO swallow.
    let missing_fc_socket = PathBuf::from("/tmp/m80-no-such-socket.sock");
    let result = restore(RestoreRequest {
        fc_socket: missing_fc_socket,
        paths: SnapshotPaths {
            vm_state: PathBuf::from("/tmp/m80-no-such-vm.snap"),
            mem: PathBuf::from("/tmp/m80-no-such-mem.snap"),
        },
        vsock_uds: PathBuf::from("/tmp/m80-no-such-vsock.sock"),
        resume: false,
    });

    assert!(result.is_err(), "restore with missing files must fail");
    let err = result.unwrap_err();
    // The error must not be opaque — it must carry context about what failed.
    let display = err.to_string();
    assert!(
        !display.is_empty(),
        "error display must not be empty"
    );
    // Must NOT be the SnapshotError::DestinationCollision or schema variant
    // (those only apply to manifest read paths). Must be Client or VsockUdsUnlink.
    assert!(
        display.contains("firecracker client") || display.contains("vsock UDS"),
        "error must be clearly classified as client or vsock-uds, got: {display}"
    );
}
