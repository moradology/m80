//! Real-KVM Bestiary stand-in for the conveyor-belt primitive set.
//!
//! Ignored by default. Run on a prepared host with:
//!
//! ```
//! sudo cargo test -p m80-firecracker -- --ignored bestiary_stand_in
//! ```
//!
//! Required environment matches `end_to_end_real_kvm.rs`: Firecracker, jailer,
//! m80-jailer-harden, kernel, rootfs, run-root, KVM access, and `mkfs.ext4`.

mod common;

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::RunDirDumpGuard;
use m80_firecracker::{
    Backend, BackendConfig, CgroupMode, FcError, HotplugDriveAttach, HotplugDriveDetach,
    NetworkPolicy, SandboxConfig, SnapshotPaths, WarmPool, WarmPoolConfig, WarmStrategy,
};
use nix::unistd::{chown, Gid, Uid};

#[test]
#[ignore = "requires KVM host with real Firecracker binary, snapshot support, and mkfs.ext4"]
fn bestiary_stand_in_attach_identity_run_destroy_no_residue() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let snap_dir = warm_snapshot_dir(&discovery, "bestiary-stand-in-snapshot");
    let prefix = "bestiary-stand-in-slot";
    let tenant_id = b"tenant-a";

    let golden_backend =
        Arc::new(Backend::new(backend_config(discovery.clone(), 1)).expect("Backend::new golden"));
    let golden = golden_backend
        .admit(bestiary_sandbox_config("bestiary-stand-in-golden", false))
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

    let pool_backend =
        Arc::new(Backend::new(backend_config(discovery.clone(), 3)).expect("Backend::new pool"));
    let pool = WarmPool::new(
        Arc::clone(&pool_backend),
        WarmPoolConfig {
            target_ready: 1,
            sandbox: bestiary_sandbox_config("bestiary-template", true),
            strategy: WarmStrategy::direct_snapshot(paths, true_request()),
            vm_id_prefix: prefix.into(),
            cpu_allocator: None,
        },
    )
    .expect("WarmPool::new");
    pool.fill_to_target_blocking().expect("prefill");

    let mut lease = pool.try_lease().expect("lease tenant slot");
    let tenant_image = create_tenant_image_in_jail(
        lease.run_dir(),
        &discovery.firecracker_bin,
        "tenant-a.ext4",
        tenant_id,
    );
    lease
        .attach_drive_verified(HotplugDriveAttach {
            slot: 0,
            path_on_host: PathBuf::from("/tenant-a.ext4"),
            mount_path: "/tenant".into(),
            identity_path: "/tenant/.tenant-identity".into(),
            expected_identity: tenant_id.to_vec(),
        })
        .expect("attach and verify tenant drive");

    let bytes_written = lease
        .write_file(
            "/tenant/warmlease-fileop.txt",
            b"fileop-ok".to_vec(),
            Some(0o600),
        )
        .expect("write through warm lease");
    assert_eq!(bytes_written, "fileop-ok".len() as u64);
    let (bytes, truncated) = lease
        .read_file("/tenant/warmlease-fileop.txt", Some(64))
        .expect("read through warm lease");
    assert!(!truncated);
    assert_eq!(String::from_utf8_lossy(&bytes), "fileop-ok");
    let stat = lease
        .stat_file("/tenant/warmlease-fileop.txt")
        .expect("stat through warm lease");
    assert_eq!(stat.size, "fileop-ok".len() as u64);
    let entries = lease.list_dir("/tenant").expect("list through warm lease");
    assert!(entries
        .iter()
        .any(|entry| entry.name == "warmlease-fileop.txt"));
    lease
        .remove_file("/tenant/warmlease-fileop.txt")
        .expect("remove through warm lease");

    let response = lease
        .exec(m80_proto::ExecRequest {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                "set -eu; \
                 test \"$(cat /tenant/.tenant-identity)\" = tenant-a; \
                 printf workload-ok > /tenant/workload-output; \
                 cat /tenant/workload-output"
                    .into(),
            ],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(10_000),
            streaming: false,
        })
        .expect("one-shot tenant workload");
    assert_eq!(response.status, m80_proto::ExecStatus::Completed);
    assert_eq!(
        response.exit_code,
        Some(0),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&response.stdout),
        String::from_utf8_lossy(&response.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&response.stdout), "workload-ok");

    pool.wait_for_ready(1, std::time::Duration::from_secs(45))
        .expect("refill clean slot");
    let mut next = pool.try_lease().expect("lease replacement slot");
    let clean = next
        .exec(m80_proto::ExecRequest {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                "test ! -e /tenant/.tenant-identity && \
                 test ! -e /tenant/workload-output && \
                 echo clean"
                    .into(),
            ],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(10_000),
            streaming: false,
        })
        .expect("replacement slot residue check");
    assert_eq!(clean.status, m80_proto::ExecStatus::Completed);
    assert_eq!(clean.exit_code, Some(0));
    assert_eq!(String::from_utf8_lossy(&clean.stdout).trim(), "clean");

    drop(pool);
    assert_no_run_dirs_with_prefix(&discovery.run_root, prefix);
    let _ = std::fs::remove_file(tenant_image);
    let _ = std::fs::remove_dir_all(&snap_dir);
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary, snapshot support, and mkfs.ext4"]
fn drive_attach_out_of_range_slot_rejects_and_refills_slot() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let snap_dir = warm_snapshot_dir(&discovery, "slot-out-of-range-snapshot");
    let prefix = "slot-out-of-range";

    let golden_backend =
        Arc::new(Backend::new(backend_config(discovery.clone(), 1)).expect("Backend::new golden"));
    let golden = golden_backend
        .admit(bestiary_sandbox_config("slot-out-of-range-golden", false))
        .expect("admit golden");
    let mut running = golden.launch().expect("launch golden");
    let _golden_dump = RunDirDumpGuard::new(running.run_dir().to_path_buf());
    let paths = snapshot_paths(&snap_dir);
    running.capture(paths.clone()).expect("capture golden");
    running
        .force_kill()
        .expect("force-kill golden")
        .delete()
        .expect("delete golden");

    let pool_backend =
        Arc::new(Backend::new(backend_config(discovery.clone(), 2)).expect("Backend::new pool"));
    let pool = WarmPool::new(
        Arc::clone(&pool_backend),
        WarmPoolConfig {
            target_ready: 1,
            sandbox: bestiary_sandbox_config("slot-out-of-range-template", true),
            strategy: WarmStrategy::direct_snapshot(paths, true_request()),
            vm_id_prefix: prefix.into(),
            cpu_allocator: None,
        },
    )
    .expect("WarmPool::new");
    pool.fill_to_target_blocking().expect("prefill");

    let mut lease = pool.try_lease().expect("lease tenant slot");
    let run_dir = lease.run_dir().to_path_buf();
    let firecracker_pid = firecracker_pid(&run_dir);
    let err = lease
        .attach_drive_verified(HotplugDriveAttach {
            slot: 1,
            path_on_host: PathBuf::from("/not-used.ext4"),
            mount_path: "/tenant".into(),
            identity_path: "/tenant/.tenant-identity".into(),
            expected_identity: b"tenant".to_vec(),
        })
        .expect_err("out-of-range slot must reject and discard the warm slot");

    assert!(
        matches!(err, FcError::Config(_)),
        "expected config error for out-of-range hotplug slot, got {err:?}"
    );
    wait_dead(firecracker_pid);
    assert!(
        !run_dir.exists(),
        "out-of-range attach must delete discarded run dir: {}",
        run_dir.display()
    );
    pool.wait_for_ready(1, Duration::from_secs(45))
        .expect("pool refills after out-of-range discard");
    assert_eq!(pool.snapshot().ready, 1);

    drop(pool);
    let _ = std::fs::remove_dir_all(&snap_dir);
    assert_no_run_dirs_with_prefix(&discovery.run_root, prefix);
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary, snapshot support, and mkfs.ext4"]
fn drive_attach_detach_attach_reuses_slot_placeholder_lifecycle() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let snap_dir = warm_snapshot_dir(&discovery, "attach-detach-attach-snapshot");
    let prefix = "attach-detach-attach-slot";

    let golden_backend =
        Arc::new(Backend::new(backend_config(discovery.clone(), 1)).expect("Backend::new golden"));
    let golden = golden_backend
        .admit(bestiary_sandbox_config(
            "attach-detach-attach-golden",
            false,
        ))
        .expect("admit golden");
    let mut running = golden.launch().expect("launch golden");
    let _golden_dump = RunDirDumpGuard::new(running.run_dir().to_path_buf());
    let paths = snapshot_paths(&snap_dir);
    running.capture(paths.clone()).expect("capture golden");
    running
        .force_kill()
        .expect("force-kill golden")
        .delete()
        .expect("delete golden");

    let pool_backend =
        Arc::new(Backend::new(backend_config(discovery.clone(), 2)).expect("Backend::new pool"));
    let pool = WarmPool::new(
        Arc::clone(&pool_backend),
        WarmPoolConfig {
            target_ready: 1,
            sandbox: bestiary_sandbox_config("attach-detach-attach-template", false),
            strategy: WarmStrategy::direct_snapshot(paths, true_request()),
            vm_id_prefix: prefix.into(),
            cpu_allocator: None,
        },
    )
    .expect("WarmPool::new");
    pool.fill_to_target_blocking().expect("prefill");

    let mut lease = pool.try_lease().expect("lease tenant slot");
    let first_image = create_tenant_image_in_jail(
        lease.run_dir(),
        &discovery.firecracker_bin,
        "tenant-first.ext4",
        b"tenant-first",
    );
    let second_image = create_tenant_image_in_jail(
        lease.run_dir(),
        &discovery.firecracker_bin,
        "tenant-second.ext4",
        b"tenant-second",
    );

    lease
        .attach_drive_verified(HotplugDriveAttach {
            slot: 0,
            path_on_host: PathBuf::from("/tenant-first.ext4"),
            mount_path: "/tenant".into(),
            identity_path: "/tenant/.tenant-identity".into(),
            expected_identity: b"tenant-first".to_vec(),
        })
        .expect("attach first tenant drive");
    lease
        .detach_drive(HotplugDriveDetach {
            slot: 0,
            mount_path: "/tenant".into(),
        })
        .expect("detach first tenant drive");
    lease
        .attach_drive_verified(HotplugDriveAttach {
            slot: 0,
            path_on_host: PathBuf::from("/tenant-second.ext4"),
            mount_path: "/tenant".into(),
            identity_path: "/tenant/.tenant-identity".into(),
            expected_identity: b"tenant-second".to_vec(),
        })
        .expect("attach second tenant drive");

    let (bytes, truncated) = lease
        .read_file("/tenant/.tenant-identity", Some(64))
        .expect("read second tenant identity through warm lease");
    assert!(!truncated);
    assert_eq!(String::from_utf8_lossy(&bytes), "tenant-second");

    lease.discard().expect("discard lease");
    pool.wait_for_ready(1, Duration::from_secs(45))
        .expect("pool refills after attach-detach-attach");
    drop(pool);
    assert_no_run_dirs_with_prefix(&discovery.run_root, prefix);
    let _ = std::fs::remove_file(first_image);
    let _ = std::fs::remove_file(second_image);
    let _ = std::fs::remove_dir_all(&snap_dir);
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary, snapshot support, and mkfs.ext4"]
fn drive_attach_identity_mismatch_kills_vm_and_refills_slot() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let snap_dir = warm_snapshot_dir(&discovery, "identity-mismatch-snapshot");
    let prefix = "identity-mismatch-slot";
    let actual_tenant_id = b"tenant-a";
    let expected_tenant_id = b"tenant-b";

    let golden_backend =
        Arc::new(Backend::new(backend_config(discovery.clone(), 1)).expect("Backend::new golden"));
    let golden = golden_backend
        .admit(bestiary_sandbox_config("identity-mismatch-golden", false))
        .expect("admit golden");
    let mut running = golden.launch().expect("launch golden");
    let _golden_dump = RunDirDumpGuard::new(running.run_dir().to_path_buf());
    let paths = snapshot_paths(&snap_dir);
    running.capture(paths.clone()).expect("capture golden");
    running
        .force_kill()
        .expect("force-kill golden")
        .delete()
        .expect("delete golden");

    let pool_backend =
        Arc::new(Backend::new(backend_config(discovery.clone(), 2)).expect("Backend::new pool"));
    let pool = WarmPool::new(
        Arc::clone(&pool_backend),
        WarmPoolConfig {
            target_ready: 1,
            sandbox: bestiary_sandbox_config("identity-mismatch-template", true),
            strategy: WarmStrategy::direct_snapshot(paths, true_request()),
            vm_id_prefix: prefix.into(),
            cpu_allocator: None,
        },
    )
    .expect("WarmPool::new");
    pool.fill_to_target_blocking().expect("prefill");

    let mut lease = pool.try_lease().expect("lease tenant slot");
    let run_dir = lease.run_dir().to_path_buf();
    let _slot_dump = RunDirDumpGuard::new(run_dir.clone());
    let firecracker_pid = firecracker_pid(&run_dir);
    let tenant_image = create_tenant_image_in_jail(
        &run_dir,
        &discovery.firecracker_bin,
        "tenant-mismatch.ext4",
        actual_tenant_id,
    );

    let err = lease
        .attach_drive_verified(HotplugDriveAttach {
            slot: 0,
            path_on_host: PathBuf::from("/tenant-mismatch.ext4"),
            mount_path: "/tenant".into(),
            identity_path: "/tenant/.tenant-identity".into(),
            expected_identity: expected_tenant_id.to_vec(),
        })
        .expect_err("identity mismatch must reject and discard the warm slot");

    assert!(
        matches!(
            err,
            FcError::TenantIdentityMismatch {
                expected_len,
                actual_len,
                ..
            } if expected_len == expected_tenant_id.len() && actual_len == actual_tenant_id.len()
        ),
        "expected TenantIdentityMismatch, got {err:?}"
    );
    wait_dead(firecracker_pid);
    assert!(
        !run_dir.exists(),
        "identity mismatch must delete the discarded run dir: {}",
        run_dir.display()
    );

    pool.wait_for_ready(1, Duration::from_secs(45))
        .expect("pool refills after identity-mismatch discard");
    let snapshot = pool.snapshot();
    assert_eq!(snapshot.ready, 1, "pool should hold one replacement slot");
    assert_eq!(snapshot.leased, 0, "mismatched lease should be released");

    drop(lease);
    drop(pool);
    assert_no_run_dirs_with_prefix(&discovery.run_root, prefix);
    let _ = std::fs::remove_file(tenant_image);
    let _ = std::fs::remove_dir_all(&snap_dir);
}

fn backend_config(discovery: m80_preflight::Discovery, max_concurrent_vms: u32) -> BackendConfig {
    let run_root = discovery.run_root.clone();
    BackendConfig::builder(discovery)
        .max_concurrent_vms(max_concurrent_vms)
        .run_root(run_root)
        .jail_uid(3000)
        .jail_gid(3000)
        .cgroup_mode(CgroupMode::Disabled)
        .build()
}

fn bestiary_sandbox_config(vm_id: impl Into<String>, one_shot: bool) -> SandboxConfig {
    SandboxConfig {
        vm_id: Some(vm_id.into()),
        workspace: None,
        network: NetworkPolicy::NoEgress,
        vcpu_count: Some(1),
        mem_size_mib: Some(512),
        cpuset_cpus: None,
        cpu_template: None,
        drive_cache_type: None,
        boot_args: None,
        overlay_size_bytes: 512 * 1024 * 1024,
        idle_timeout: None,
        daemonize: false,
        request_id: None,
        pmem_layers: Vec::new(),
        preallocated_drive_slots: 1,
        one_shot,
    }
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

fn create_tenant_image_in_jail(
    run_dir: &Path,
    firecracker_bin: &Path,
    image_name: &str,
    tenant_id: &[u8],
) -> PathBuf {
    let jail_root = m80_jailer::jail_root_path(run_dir, firecracker_bin);
    let image_path = jail_root.join(image_name);
    let source = tempfile::tempdir().expect("tenant source dir");
    std::fs::write(source.path().join(".tenant-identity"), tenant_id).expect("tenant identity");
    let workload_output = source.path().join("workload-output");
    std::fs::write(&workload_output, b"stale-host-value").expect("seed workload file");
    let mut perms = std::fs::metadata(&workload_output)
        .expect("workload file metadata")
        .permissions();
    perms.set_mode(0o666);
    std::fs::set_permissions(&workload_output, perms).expect("make workload file writable");

    let image = std::fs::File::create(&image_path).expect("create tenant ext4 image");
    image
        .set_len(64 * 1024 * 1024)
        .expect("size tenant ext4 image");
    let status = Command::new("mkfs.ext4")
        .arg("-F")
        .arg("-q")
        .arg("-d")
        .arg(source.path())
        .arg(&image_path)
        .status()
        .expect("run mkfs.ext4");
    assert!(status.success(), "mkfs.ext4 failed with {status}");
    chown(
        &image_path,
        Some(Uid::from_raw(3000)),
        Some(Gid::from_raw(3000)),
    )
    .expect("chown tenant ext4 image to jail uid/gid");
    image_path
}

fn firecracker_pid(run_dir: &Path) -> u32 {
    let state_path = run_dir.join("jailer-state.json");
    let state: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&state_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", state_path.display())),
    )
    .expect("jailer-state.json parses");
    state["firecracker_pid"]
        .as_u64()
        .unwrap_or_else(|| panic!("firecracker_pid missing from {}", state_path.display()))
        as u32
}

fn wait_dead(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if !process_is_live(pid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(
        !process_is_live(pid),
        "firecracker pid {pid} should be gone after identity mismatch"
    );
}

fn process_is_live(pid: u32) -> bool {
    let proc_dir = PathBuf::from(format!("/proc/{pid}"));
    match std::fs::read_to_string(proc_dir.join("stat")) {
        Ok(stat) => !matches!(proc_stat_state(&stat), Some('Z' | 'X')),
        Err(_) => proc_dir.exists(),
    }
}

fn proc_stat_state(stat: &str) -> Option<char> {
    let (_comm, after_comm) = stat.rsplit_once(") ")?;
    after_comm.chars().next()
}

fn assert_no_run_dirs_with_prefix(run_root: &Path, prefix: &str) {
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
