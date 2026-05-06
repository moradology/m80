//! Warm-pool unit and real-KVM integration coverage.

mod common;
use common::RunDirDumpGuard;

use std::sync::Arc;

use m80_firecracker::{
    Backend, BackendConfig, BlankVmResetDecision, BlankVmResetDiscardReason, BlankVmResetEvidence,
    CgroupMode, FcError, NetworkPolicy, SandboxConfig, SnapshotPaths, WarmPool, WarmPoolConfig,
};

#[test]
fn reset_evidence_requires_every_input() {
    let evidence = BlankVmResetEvidence {
        ownership_and_lease: true,
        boot_identity: true,
        no_workspace_id_attached: true,
        no_run_id_attached: true,
        empty_guest_workspace: true,
        clean_run_root_surface: true,
        clean_diagnostics: true,
        post_reset_guestd_probe: true,
    };
    assert_eq!(evidence.decision(), Ok(BlankVmResetDecision::Reusable));
}

#[test]
fn reset_evidence_does_not_infer_from_partial_truth() {
    let evidence = BlankVmResetEvidence {
        ownership_and_lease: true,
        boot_identity: true,
        no_workspace_id_attached: true,
        no_run_id_attached: true,
        empty_guest_workspace: true,
        clean_run_root_surface: true,
        clean_diagnostics: true,
        post_reset_guestd_probe: false,
    };
    assert_eq!(
        evidence.decision(),
        Err(BlankVmResetDiscardReason::PostResetGuestdProbe)
    );
}

fn make_backend_config(
    discovery: m80_preflight::Discovery,
    max_concurrent_vms: u32,
) -> BackendConfig {
    let run_root = discovery.run_root.clone();
    BackendConfig {
        discovery,
        max_concurrent_vms,
        run_root,
        jail_uid: 3000,
        jail_gid: 3000,
        cgroup_mode: CgroupMode::Disabled,
    }
}

fn sandbox_config(vm_id: impl Into<String>) -> SandboxConfig {
    SandboxConfig {
        vm_id: Some(vm_id.into()),
        workspace: None,
        network: NetworkPolicy::NoEgress,
        vcpu_count: Some(1),
        mem_size_mib: Some(512),
        boot_args: None,
        overlay_size_bytes: 128 * 1024 * 1024,
        idle_timeout: None,
        request_id: None,
    }
}

fn snapshot_paths(dir: &std::path::Path) -> SnapshotPaths {
    std::fs::create_dir_all(dir).expect("create snapshot dir");
    SnapshotPaths {
        vm_state: dir.join("vm.snap"),
        mem: dir.join("mem.snap"),
    }
}

#[test]
fn empty_pool_returns_pool_empty_without_cold_boot_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let discovery = common::fake_discovery(dir.path());
    let backend = Arc::new(Backend::new(make_backend_config(discovery, 1)).expect("Backend::new"));
    let pool = WarmPool::new(
        backend,
        WarmPoolConfig {
            target_ready: 1,
            snapshot: snapshot_paths(dir.path()),
            sandbox: sandbox_config("template"),
            ready_probe: true_request(),
            vm_id_prefix: "empty".into(),
        },
    )
    .expect("WarmPool::new");

    let err = match pool.try_lease() {
        Ok(_) => panic!("empty pool must not lease"),
        Err(err) => err,
    };
    assert!(
        matches!(err, FcError::PoolEmpty { target_ready: 1 }),
        "expected PoolEmpty, got {err:?}"
    );
}

#[test]
fn workspace_backed_pool_config_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let discovery = common::fake_discovery(dir.path());
    let backend = Arc::new(Backend::new(make_backend_config(discovery, 1)).expect("Backend::new"));
    let mut sandbox = sandbox_config("template");
    sandbox.workspace = Some(dir.path().join("workspace"));

    let err = match WarmPool::new(
        backend,
        WarmPoolConfig {
            target_ready: 1,
            snapshot: snapshot_paths(dir.path()),
            sandbox,
            ready_probe: true_request(),
            vm_id_prefix: "workspace".into(),
        },
    ) {
        Ok(_) => panic!("workspace-backed pool config must fail"),
        Err(err) => err,
    };
    assert!(
        matches!(err, FcError::Config(_)),
        "workspace config must fail closed, got {err:?}"
    );
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn warm_pool_allocates_pre_restored_slot_and_refills_after_discard() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let snap_dir = discovery.run_root.join("warm-pool-test-snapshot");

    let golden_backend = Arc::new(
        Backend::new(make_backend_config(discovery.clone(), 1)).expect("Backend::new golden"),
    );
    let golden = golden_backend
        .admit(sandbox_config("warm-pool-golden"))
        .expect("admit golden");
    let mut running = golden.launch().expect("launch golden");
    let _dump = RunDirDumpGuard::new(running.run_dir().to_path_buf());
    let paths = snapshot_paths(&snap_dir);
    running.capture(paths.clone()).expect("capture golden");
    running
        .force_kill()
        .expect("force-kill golden")
        .delete()
        .expect("delete golden");

    let pool_backend = Arc::new(
        Backend::new(make_backend_config(discovery.clone(), 3)).expect("Backend::new pool"),
    );
    let pool = WarmPool::new(
        Arc::clone(&pool_backend),
        WarmPoolConfig {
            target_ready: 1,
            snapshot: paths.clone(),
            sandbox: sandbox_config("warm-template"),
            ready_probe: true_request(),
            vm_id_prefix: "warm-pool-slot".into(),
        },
    )
    .expect("WarmPool::new");
    pool.fill_to_target_blocking().expect("prefill");
    assert_eq!(pool.snapshot().ready, 1);

    let mut lease = pool.try_lease().expect("lease");
    assert_eq!(lease.reset_decision(), BlankVmResetDecision::Discard);
    let resp = lease
        .exec(m80_proto::ExecRequest {
            program: "/bin/echo".into(),
            args: vec!["warm".into()],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        })
        .expect("exec warm lease");
    assert_eq!(resp.exit_code, Some(0));
    assert_eq!(String::from_utf8_lossy(&resp.stdout).trim(), "warm");
    lease.discard().expect("discard lease");

    pool.wait_for_ready(1, std::time::Duration::from_secs(30))
        .expect("refill");
    assert_eq!(pool.snapshot().ready, 1);
    drop(pool);
    assert_no_run_dirs_with_prefix(&discovery.run_root, "warm-pool-slot");

    let _ = std::fs::remove_dir_all(&snap_dir);
}

fn assert_no_run_dirs_with_prefix(run_root: &std::path::Path, prefix: &str) {
    let entries = std::fs::read_dir(run_root)
        .unwrap_or_else(|e| panic!("read run root {}: {e}", run_root.display()));
    let leaked = entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(prefix))
        .collect::<Vec<_>>();
    assert!(
        leaked.is_empty(),
        "warm-pool drop leaked run dirs: {leaked:?}"
    );
}

fn true_request() -> m80_proto::ExecRequest {
    m80_proto::ExecRequest {
        program: "/bin/true".into(),
        args: Vec::new(),
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(5_000),
        streaming: false,
    }
}
