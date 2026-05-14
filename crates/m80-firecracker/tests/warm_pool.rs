//! Warm-pool unit and real-KVM integration coverage.

mod common;
use common::RunDirDumpGuard;

use std::sync::{mpsc, Arc, Barrier};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use m80_firecracker::{
    Backend, BackendConfig, CgroupMode, FcError, SandboxConfig, SnapshotPaths, WarmPool,
    WarmPoolConfig, WarmPoolCpuAllocator, FIRST_LINE_MEM_SIZE_MIB, FIRST_LINE_VCPU_COUNT,
};

fn make_backend_config(
    discovery: m80_preflight::Discovery,
    max_concurrent_vms: u32,
) -> BackendConfig {
    make_backend_config_with_cgroup_mode(discovery, max_concurrent_vms, CgroupMode::Disabled)
}

fn make_backend_config_with_cgroup_mode(
    discovery: m80_preflight::Discovery,
    max_concurrent_vms: u32,
    cgroup_mode: CgroupMode,
) -> BackendConfig {
    let run_root = discovery.run_root.clone();
    BackendConfig::builder(discovery)
        .max_concurrent_vms(max_concurrent_vms)
        .run_root(run_root)
        .jail_uid(3000)
        .jail_gid(3000)
        .cgroup_mode(cgroup_mode)
        .build()
}

fn sandbox_config(vm_id: impl Into<String>) -> SandboxConfig {
    SandboxConfig {
        vcpu_count: Some(FIRST_LINE_VCPU_COUNT),
        mem_size_mib: Some(FIRST_LINE_MEM_SIZE_MIB),
        cpuset_cpus: None,
        cpu_template: None,
        overlay_size_bytes: 128 * 1024 * 1024,
        ..common::sandbox_config_with_id(vm_id)
    }
}

fn snapshot_paths(dir: &std::path::Path) -> SnapshotPaths {
    std::fs::create_dir_all(dir).expect("create snapshot dir");
    SnapshotPaths {
        vm_state: dir.join("vm.snap"),
        mem: dir.join("mem.snap"),
    }
}

fn warm_snapshot_dir(
    discovery: &m80_preflight::Discovery,
    name: impl AsRef<str>,
) -> std::path::PathBuf {
    discovery.run_root.join("warm").join(name.as_ref())
}

fn unique_name(prefix: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis()
        % 1_000_000;
    format!("{prefix}-{millis}")
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
            cpu_allocator: None,
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
            cpu_allocator: None,
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
    let snap_dir = warm_snapshot_dir(&discovery, "warm-pool-test-snapshot");

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
            snapshot: paths,
            sandbox: sandbox_config("warm-template"),
            ready_probe: true_request(),
            vm_id_prefix: "warm-pool-slot".into(),
            cpu_allocator: None,
        },
    )
    .expect("WarmPool::new");
    pool.fill_to_target_blocking().expect("prefill");
    assert_eq!(pool.snapshot().ready, 1);

    let mut lease = pool.try_lease().expect("lease");
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

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn warm_pool_empty_returns_pool_empty_error() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let suffix = unique_name("pool-empty");
    let snap_dir = warm_snapshot_dir(&discovery, format!("{suffix}-snapshot"));

    let golden_backend = Arc::new(
        Backend::new(make_backend_config(discovery.clone(), 1)).expect("Backend::new golden"),
    );
    let golden = golden_backend
        .admit(sandbox_config(format!("{suffix}-golden")))
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
        Backend::new(make_backend_config(discovery.clone(), 1)).expect("Backend::new pool"),
    );
    let pool = WarmPool::new(
        Arc::clone(&pool_backend),
        WarmPoolConfig {
            target_ready: 1,
            snapshot: paths,
            sandbox: sandbox_config(format!("{suffix}-template")),
            ready_probe: true_request(),
            vm_id_prefix: format!("{suffix}-slot"),
            cpu_allocator: None,
        },
    )
    .expect("WarmPool::new");
    pool.fill_to_target_blocking().expect("prefill");
    assert_eq!(pool.snapshot().ready, 1);

    let lease = pool
        .try_lease()
        .expect("first lease drains only ready slot");
    let err = match pool.try_lease() {
        Ok(_) => panic!("second lease must fail while the only slot is leased"),
        Err(err) => err,
    };
    assert!(
        matches!(err, FcError::PoolEmpty { target_ready: 1 }),
        "expected PoolEmpty after draining warm pool, got {err:?}"
    );
    assert_eq!(pool.snapshot().ready, 0);
    assert_eq!(pool.snapshot().leased, 1);
    lease.discard().expect("discard held lease");
    pool.wait_for_ready(1, std::time::Duration::from_secs(60))
        .expect("refill after held lease discard");
    assert_eq!(pool.snapshot().ready, 1);
    assert_eq!(pool.snapshot().leased, 0);

    drop(pool);
    let _ = std::fs::remove_dir_all(&snap_dir);
    assert_no_run_dirs_with_prefix(&discovery.run_root, &suffix);
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn warm_pool_simultaneous_lease_and_refill() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let suffix = unique_name("pool-race");
    let snap_dir = warm_snapshot_dir(&discovery, format!("{suffix}-snapshot"));

    let golden_backend = Arc::new(
        Backend::new(make_backend_config(discovery.clone(), 1)).expect("Backend::new golden"),
    );
    let golden = golden_backend
        .admit(sandbox_config(format!("{suffix}-golden")))
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
        Backend::new(make_backend_config(discovery.clone(), 4)).expect("Backend::new pool"),
    );
    let pool = Arc::new(
        WarmPool::new(
            Arc::clone(&pool_backend),
            WarmPoolConfig {
                target_ready: 2,
                snapshot: paths,
                sandbox: sandbox_config(format!("{suffix}-template")),
                ready_probe: true_request(),
                vm_id_prefix: format!("{suffix}-slot"),
                cpu_allocator: None,
            },
        )
        .expect("WarmPool::new"),
    );
    pool.fill_to_target_blocking().expect("prefill");
    assert_eq!(pool.snapshot().ready, 2);

    let start = Arc::new(Barrier::new(3));
    let release = Arc::new(Barrier::new(3));
    let (tx, rx) = mpsc::channel();
    let mut handles = Vec::new();

    for worker_id in 0..2 {
        let pool = Arc::clone(&pool);
        let start = Arc::clone(&start);
        let release = Arc::clone(&release);
        let tx = tx.clone();
        handles.push(thread::spawn(move || {
            start.wait();
            let lease = pool.try_lease();
            tx.send((worker_id, lease.is_ok()))
                .expect("send lease result");
            release.wait();
            if let Ok(lease) = lease {
                lease.discard().expect("discard simultaneous lease");
            }
        }));
    }
    drop(tx);

    start.wait();
    let mut results = vec![false; 2];
    for _ in 0..2 {
        let (worker_id, leased) = rx.recv().expect("recv lease result");
        results[worker_id] = leased;
    }
    assert_eq!(results, vec![true, true]);
    let drained = pool.snapshot();
    assert_eq!(drained.ready, 0);
    assert_eq!(drained.leased, 2);

    release.wait();
    for handle in handles {
        handle.join().expect("lease worker panicked");
    }

    pool.wait_for_ready(2, std::time::Duration::from_secs(60))
        .expect("refill after simultaneous leases");
    let refilled = pool.snapshot();
    assert_eq!(refilled.ready, 2);
    assert_eq!(refilled.leased, 0);
    assert_eq!(refilled.discarded, 2);

    drop(pool);
    let _ = std::fs::remove_dir_all(&snap_dir);
    assert_no_run_dirs_with_prefix(&discovery.run_root, &suffix);
}

#[test]
#[ignore = "requires root, writable cgroup v2, KVM, and real Firecracker binary"]
fn warm_pool_cpuset_allocator_assigns_disjoint_concurrent_slots() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let suffix = unique_name("pool-cpuset");
    let snap_dir = warm_snapshot_dir(&discovery, format!("{suffix}-snapshot"));

    let golden_backend = Arc::new(
        Backend::new(make_backend_config(discovery.clone(), 1)).expect("Backend::new golden"),
    );
    let golden = golden_backend
        .admit(sandbox_config(format!("{suffix}-golden")))
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
        Backend::new(make_backend_config_with_cgroup_mode(
            discovery.clone(),
            4,
            CgroupMode::UnifiedV2,
        ))
        .expect("Backend::new pool"),
    );
    let pool = Arc::new(
        WarmPool::new(
            Arc::clone(&pool_backend),
            WarmPoolConfig {
                target_ready: 2,
                snapshot: paths,
                sandbox: sandbox_config(format!("{suffix}-template")),
                ready_probe: true_request(),
                vm_id_prefix: format!("{suffix}-slot"),
                cpu_allocator: Some(WarmPoolCpuAllocator {
                    first_cpu: 0,
                    cpus_per_slot: 1,
                }),
            },
        )
        .expect("WarmPool::new"),
    );
    pool.fill_to_target_blocking().expect("prefill");

    let start = Arc::new(Barrier::new(3));
    let (tx, rx) = mpsc::channel();
    let mut handles = Vec::new();
    for worker_id in 0..2 {
        let pool = Arc::clone(&pool);
        let start = Arc::clone(&start);
        let tx = tx.clone();
        handles.push(thread::spawn(move || {
            start.wait();
            let lease = pool.try_lease().expect("lease pinned slot");
            let cpuset = read_lease_cpuset(&lease);
            tx.send((worker_id, cpuset)).expect("send cpuset");
            lease.discard().expect("discard pinned lease");
        }));
    }
    drop(tx);
    start.wait();

    let mut ranges = vec![String::new(), String::new()];
    for _ in 0..2 {
        let (worker_id, cpuset) = rx.recv().expect("recv cpuset");
        ranges[worker_id] = cpuset;
    }
    for handle in handles {
        handle.join().expect("cpuset lease worker panicked");
    }

    ranges.sort();
    ranges.dedup();
    assert_eq!(ranges, vec!["0", "1"]);
    pool.wait_for_ready(2, std::time::Duration::from_secs(60))
        .expect("refill after pinned leases");
    drop(pool);
    let _ = std::fs::remove_dir_all(&snap_dir);
    assert_no_run_dirs_with_prefix(&discovery.run_root, &suffix);
}

fn assert_no_run_dirs_with_prefix(run_root: &std::path::Path, prefix: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let entries = std::fs::read_dir(run_root)
            .unwrap_or_else(|e| panic!("read run root {}: {e}", run_root.display()));
        let leaked = entries
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(prefix))
            .collect::<Vec<_>>();
        if leaked.is_empty() {
            return;
        }
        if std::time::Instant::now() >= deadline {
            panic!("warm-pool drop leaked run dirs: {leaked:?}");
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn read_lease_cpuset(lease: &m80_firecracker::WarmLease) -> String {
    let cgroup_path =
        std::fs::read_to_string(lease.run_dir().join("cgroup-path.txt")).expect("read cgroup path");
    std::fs::read_to_string(std::path::Path::new(cgroup_path.trim()).join("cpuset.cpus"))
        .expect("read lease cpuset")
        .trim()
        .to_owned()
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
