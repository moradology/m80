use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;

use super::*;
use crate::types::{BackendConfig, CgroupMode};
use m80_proto::ExecRequest;
use m80_snapshot::SnapshotPaths;
use m80_snapshot_template::{
    HookSpecSet, TemplateFingerprint, TemplateInputs, TemplateManifest, TemplateRestoreLayout,
    TemplateStore, TemplateStoreError,
};

const FC_VERSION: &str = "v1.15.1";

#[test]
fn warm_snapshot_verify_accepts_matching_manifest() {
    let dir = tempfile::tempdir().expect("snapshot dir");
    let paths = write_snapshot_pair(dir.path());
    m80_snapshot::write_snapshot_manifest(&paths, FC_VERSION).expect("write manifest");

    verify_warm_snapshot(&paths, FC_VERSION).expect("matching warm snapshot");
}

#[test]
fn warm_snapshot_verify_rejects_tampered_memory_before_slot_launch() {
    let dir = tempfile::tempdir().expect("snapshot dir");
    let paths = write_snapshot_pair(dir.path());
    m80_snapshot::write_snapshot_manifest(&paths, FC_VERSION).expect("write manifest");
    std::fs::write(&paths.mem, b"tampered-memory").expect("tamper memory");

    let err = verify_warm_snapshot(&paths, FC_VERSION)
        .expect_err("tampered warm snapshot must be rejected");

    assert!(
        matches!(
            err,
            FcError::Snapshot(m80_snapshot::SnapshotError::ArtifactMismatch {
                kind: m80_snapshot::ArtifactKind::Memory,
                ..
            })
        ),
        "expected memory artifact mismatch, got {err:?}"
    );
}

#[test]
fn warm_pool_fill_rejects_tampered_snapshot_before_admission() {
    let run_root = tempfile::tempdir().expect("run root");
    let snapshot_dir = run_root.path().join("warm/snapshot");
    std::fs::create_dir_all(&snapshot_dir).expect("snapshot dir");
    let paths = write_snapshot_pair(&snapshot_dir);
    let discovery = fake_discovery(run_root.path());
    let expected_firecracker_version = discovery.manifest.expected_firecracker_version.clone();
    m80_snapshot::write_snapshot_manifest(&paths, &expected_firecracker_version)
        .expect("write manifest");
    std::fs::write(&paths.mem, b"tampered-memory").expect("tamper memory");

    let backend = Arc::new(
        Backend::new(
            BackendConfig::builder(discovery)
                .max_concurrent_vms(1)
                .run_root(run_root.path())
                .cgroup_mode(CgroupMode::Disabled)
                .build(),
        )
        .expect("backend"),
    );
    let pool = WarmPool::new(
        backend,
        WarmPoolConfig {
            target_ready: 1,
            sandbox: SandboxConfig::default(),
            strategy: WarmStrategy::direct_snapshot(paths, true_request()),
            vm_id_prefix: "tampered-warm".to_owned(),
            cpu_allocator: None,
        },
    )
    .expect("warm pool");

    let err = pool
        .fill_to_target_blocking()
        .expect_err("tampered snapshot must fail before launch");

    assert!(
        matches!(
            err,
            FcError::Snapshot(m80_snapshot::SnapshotError::ArtifactMismatch {
                kind: m80_snapshot::ArtifactKind::Memory,
                ..
            })
        ),
        "expected memory artifact mismatch, got {err:?}"
    );
    let snapshot = pool.snapshot();
    assert_eq!(snapshot.ready, 0);
    assert_eq!(snapshot.filling, 0);
    assert_eq!(snapshot.fill_attempts_total, 1);
    assert_eq!(snapshot.fill_failures_total, 1);
    assert_eq!(snapshot.consecutive_fill_errors, 1);
    assert_eq!(snapshot.discarded, 1);
}

#[test]
fn target_ready_above_admission_limit_is_rejected() {
    let run_root = tempfile::tempdir().expect("run root");
    let discovery = fake_discovery(run_root.path());
    let backend = Arc::new(
        Backend::new(
            BackendConfig::builder(discovery)
                .max_concurrent_vms(1)
                .run_root(run_root.path())
                .cgroup_mode(CgroupMode::Disabled)
                .build(),
        )
        .expect("backend"),
    );

    let err = match WarmPool::new(
        backend,
        WarmPoolConfig {
            target_ready: 2,
            sandbox: SandboxConfig::default(),
            strategy: WarmStrategy::direct_snapshot(
                SnapshotPaths {
                    vm_state: run_root.path().join("vm.snap"),
                    mem: run_root.path().join("mem.snap"),
                },
                true_request(),
            ),
            vm_id_prefix: "too-large".to_owned(),
            cpu_allocator: None,
        },
    ) {
        Ok(_) => panic!("target_ready above max_concurrent_vms must fail closed"),
        Err(err) => err,
    };

    assert!(
        matches!(err, FcError::Config(ConfigError::InvalidValue { field, .. }) if field == "warm_pool.target_ready"),
        "oversized target_ready must be a config error, got {err:?}"
    );
}

#[test]
fn set_target_ready_rejects_values_above_admission_limit() {
    let run_root = tempfile::tempdir().expect("run root");
    let discovery = fake_discovery(run_root.path());
    let backend = Arc::new(
        Backend::new(
            BackendConfig::builder(discovery)
                .max_concurrent_vms(1)
                .run_root(run_root.path())
                .cgroup_mode(CgroupMode::Disabled)
                .build(),
        )
        .expect("backend"),
    );
    let pool = WarmPool::new(
        backend,
        WarmPoolConfig {
            target_ready: 1,
            sandbox: SandboxConfig::default(),
            strategy: WarmStrategy::direct_snapshot(
                SnapshotPaths {
                    vm_state: run_root.path().join("vm.snap"),
                    mem: run_root.path().join("mem.snap"),
                },
                true_request(),
            ),
            vm_id_prefix: "resize-too-large".to_owned(),
            cpu_allocator: None,
        },
    )
    .expect("warm pool");

    let err = pool
        .set_target_ready(NonZeroUsize::new(2).unwrap())
        .expect_err("resize above admission limit must fail closed");

    assert!(
        matches!(err, FcError::Config(ConfigError::InvalidValue { field, .. }) if field == "warm_pool.target_ready"),
        "oversized resize must be a config error, got {err:?}"
    );
    assert_eq!(pool.snapshot().target_ready, 1);
}

#[test]
fn set_target_ready_rejects_values_above_initial_cpu_allocator_capacity() {
    let run_root = tempfile::tempdir().expect("run root");
    let discovery = fake_discovery(run_root.path());
    let backend = Arc::new(
        Backend::new(
            BackendConfig::builder(discovery)
                .max_concurrent_vms(2)
                .run_root(run_root.path())
                .cgroup_mode(CgroupMode::Disabled)
                .build(),
        )
        .expect("backend"),
    );
    let pool = WarmPool::new(
        backend,
        WarmPoolConfig {
            target_ready: 1,
            sandbox: SandboxConfig::default(),
            strategy: WarmStrategy::direct_snapshot(
                SnapshotPaths {
                    vm_state: run_root.path().join("vm.snap"),
                    mem: run_root.path().join("mem.snap"),
                },
                true_request(),
            ),
            vm_id_prefix: "resize-cpuset-capacity".to_owned(),
            cpu_allocator: Some(WarmPoolCpuAllocator {
                first_cpu: 0,
                cpus_per_slot: 1,
            }),
        },
    )
    .expect("warm pool");

    let err = pool
        .set_target_ready(NonZeroUsize::new(2).unwrap())
        .expect_err("resize above initial cpuset capacity must fail closed");

    assert!(
        matches!(err, FcError::Config(ConfigError::InvalidValue { field, ref reason }) if field == "warm_pool.target_ready" && reason.contains("initial cpuset allocator capacity")),
        "oversized cpuset resize must be a config error, got {err:?}"
    );
    assert_eq!(pool.snapshot().target_ready, 1);
}

#[test]
fn fill_worker_panic_rolls_back_filling_count() {
    let run_root = tempfile::tempdir().expect("run root");
    let discovery = fake_discovery(run_root.path());
    let backend = Arc::new(
        Backend::new(
            BackendConfig::builder(discovery)
                .max_concurrent_vms(1)
                .run_root(run_root.path())
                .cgroup_mode(CgroupMode::Disabled)
                .build(),
        )
        .expect("backend"),
    );
    let pool = WarmPool::new(
        backend,
        WarmPoolConfig {
            target_ready: 1,
            sandbox: SandboxConfig::default(),
            strategy: WarmStrategy::direct_snapshot(
                SnapshotPaths {
                    vm_state: run_root.path().join("vm.snap"),
                    mem: run_root.path().join("mem.snap"),
                },
                true_request(),
            ),
            vm_id_prefix: "panic-warm".to_owned(),
            cpu_allocator: None,
        },
    )
    .expect("warm pool");

    trigger_next_launch_slot_panic_for_test(&pool);
    pool.start_background_fill();

    pool.wait_for_idle(Duration::from_secs(2))
        .expect("panic in fill worker must not leave filling stuck");
    let snapshot = pool.snapshot();
    assert_eq!(snapshot.filling, 0);
    assert_eq!(snapshot.fill_failures_total, 1);
    assert_eq!(snapshot.discarded, 1);
    let state = pool.inner.state.lock().unwrap_or_else(|p| p.into_inner());
    assert_eq!(
        state.last_fill_error.as_deref(),
        Some("panic: injected warm-pool launch_slot panic")
    );
}

#[test]
fn snapshot_restore_fingerprint_mismatch_records_fill_failure() {
    let run_root = tempfile::tempdir().expect("run root");
    let discovery = fake_discovery(run_root.path());
    let backend = Arc::new(
        Backend::new(
            BackendConfig::builder(discovery)
                .max_concurrent_vms(1)
                .run_root(run_root.path())
                .cgroup_mode(CgroupMode::Disabled)
                .build(),
        )
        .expect("backend"),
    );
    let sandbox = SandboxConfig::default();
    let hooks = HookSpecSet::empty();
    let inputs =
        template_build::template_inputs_for_current_host(&backend, &sandbox, hooks.clone())
            .expect("inputs");
    let fingerprint = TemplateFingerprint::compute(&inputs);
    let store =
        Arc::new(TemplateStore::create(run_root.path().join("templates"), 4).expect("store"));
    commit_fake_template(&store, inputs.clone()).expect("fake template");
    tamper_template_manifest_inputs(&store, &fingerprint, stale_inputs(&inputs))
        .expect("tamper manifest");
    let pool = WarmPool::new(
        backend,
        WarmPoolConfig {
            target_ready: 1,
            sandbox,
            strategy: WarmStrategy::snapshot_restore(store, hooks, true_request()),
            vm_id_prefix: "template-mismatch".to_owned(),
            cpu_allocator: None,
        },
    )
    .expect("warm pool");

    let err = pool
        .fill_to_target_blocking()
        .expect_err("mismatched template must fail fill");

    assert!(
        matches!(
            err,
            FcError::TemplateStore(TemplateStoreError::FingerprintMismatch {
                stored,
                live
            }) if stored == fingerprint && live != fingerprint
        ),
        "expected template fingerprint mismatch, got {err:?}"
    );
    let snapshot = pool.snapshot();
    assert_eq!(snapshot.ready, 0);
    assert_eq!(snapshot.filling, 0);
    assert_eq!(snapshot.fill_attempts_total, 1);
    assert_eq!(snapshot.fill_failures_total, 1);
    assert_eq!(snapshot.consecutive_fill_errors, 1);
}

#[test]
fn fill_duration_samples_are_recorded_in_order_and_drained() {
    let mut state = empty_state();
    state.last_fill_error = Some("previous failure".to_owned());
    state.consecutive_fill_errors = 1;

    state.record_fill_success(10);
    state.record_fill_success(20);

    assert_eq!(state.take_fill_duration_samples_us(), vec![10, 20]);
    assert!(state.take_fill_duration_samples_us().is_empty());
    assert_eq!(state.last_fill_error, None);
    assert_eq!(state.consecutive_fill_errors, 0);
}

#[test]
fn fill_duration_samples_drop_oldest_when_retention_is_full() {
    let mut state = empty_state();

    for sample in 0..(MAX_FILL_DURATION_SAMPLES + 2) {
        state.record_fill_success(sample as u64);
    }

    let samples = state.take_fill_duration_samples_us();
    assert_eq!(samples.len(), MAX_FILL_DURATION_SAMPLES);
    assert_eq!(samples[0], 2);
    assert_eq!(
        *samples.last().expect("retained final sample"),
        (MAX_FILL_DURATION_SAMPLES + 1) as u64
    );
}

fn empty_state() -> WarmPoolState {
    WarmPoolState {
        ready: std::collections::VecDeque::new(),
        filling: 0,
        leased: 0,
        discarded: 0,
        free_cpuset_cpus: std::collections::VecDeque::new(),
        last_fill_error: None,
        consecutive_fill_errors: 0,
        fill_attempts_total: 0,
        fill_failures_total: 0,
        lease_acquired_total: 0,
        lease_returned_total: 0,
        fill_duration_samples_us: std::collections::VecDeque::new(),
    }
}

fn trigger_next_launch_slot_panic_for_test(pool: &WarmPool) {
    pool.inner
        .panic_next_launch_slot
        .store(true, Ordering::SeqCst);
}

fn true_request() -> ExecRequest {
    ExecRequest {
        program: "/bin/true".to_owned(),
        args: Vec::new(),
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(5_000),
        streaming: false,
    }
}

fn write_snapshot_pair(dir: &std::path::Path) -> SnapshotPaths {
    let paths = SnapshotPaths {
        vm_state: dir.join("vm.snap"),
        mem: dir.join("mem.snap"),
    };
    std::fs::write(&paths.vm_state, b"vm-state").expect("write vm snapshot");
    std::fs::write(&paths.mem, b"memory").expect("write memory snapshot");
    paths
}

fn commit_fake_template(
    store: &TemplateStore,
    inputs: TemplateInputs,
) -> Result<m80_snapshot_template::PinnedTemplate, FcError> {
    let layout = template_build::restore_layout_for_inputs(&inputs)?;
    commit_fake_template_with_layout(store, inputs, layout)
}

fn commit_fake_template_with_layout(
    store: &TemplateStore,
    inputs: TemplateInputs,
    layout: TemplateRestoreLayout,
) -> Result<m80_snapshot_template::PinnedTemplate, FcError> {
    let plan = store.reserve(inputs, layout)?;
    std::fs::write(&plan.body_paths().vm_state, b"vm").expect("write vm");
    std::fs::write(&plan.body_paths().mem, b"mem").expect("write mem");
    Ok(store.commit(plan)?)
}

fn tamper_template_manifest_inputs(
    store: &TemplateStore,
    fingerprint: &TemplateFingerprint,
    inputs: TemplateInputs,
) -> Result<(), TemplateStoreError> {
    let manifest_path = store.template_dir(fingerprint).join("manifest.json");
    let mut manifest = TemplateManifest::read(&manifest_path)?;
    manifest.inputs = inputs;
    manifest.write(&manifest_path)
}

fn stale_inputs(base: &TemplateInputs) -> TemplateInputs {
    TemplateInputs::new(
        "stale-host-kernel",
        base.firecracker_version(),
        base.guest_kernel_digest().clone(),
        base.pmem_image_digest_set().to_vec(),
        base.post_init_state_digest().clone(),
        base.hook_spec_set().clone(),
    )
    .expect("stale inputs")
}

fn fake_discovery(run_root: &std::path::Path) -> m80_preflight::Discovery {
    let rootfs = tempfile::NamedTempFile::new().expect("fake rootfs");
    let rootfs_path = rootfs.path().to_path_buf();
    let rootfs_file = rootfs.reopen().expect("fake rootfs fd");
    let net_helper_bin = fake_net_helper(run_root);
    m80_preflight::Discovery {
        firecracker_bin: "/tmp/firecracker".into(),
        firecracker_seccomp_filter: "/tmp/firecracker-seccomp-filter.bin".into(),
        jailer_bin: "/tmp/jailer".into(),
        firecracker_version: "v1.0.0".to_owned(),
        jailer_version: "v1.0.0".to_owned(),
        jailer_harden_bin: "/tmp/m80-jailer-harden".into(),
        net_helper_bin,
        kernel: "/tmp/vmlinux".into(),
        rootfs: "/tmp/rootfs.ext4".into(),
        pinned_rootfs: m80_preflight::PinnedRootfs::from_file(rootfs_path, rootfs_file),
        manifest: m80_image_manifest::Manifest::new(
            "/tmp/m80-guestd".into(),
            "0".repeat(64),
            "v1.0.0".to_owned(),
            52,
            m80_image_manifest::ImageKind::Minimal,
            "/tmp/vmlinux".into(),
            "1".repeat(64),
            m80_image_manifest::KernelKind::Stock,
            None,
            "/tmp/rootfs.ext4".into(),
            "2".repeat(64),
            "M80_READY".to_owned(),
            m80_image_manifest::RootfsFormat::Ext4,
            None,
            None,
        ),
        run_root: run_root.to_path_buf(),
        privilege: m80_preflight::PrivilegeStatus::Root,
        report: Vec::new(),
    }
}

fn fake_net_helper(run_root: &std::path::Path) -> std::path::PathBuf {
    std::fs::create_dir_all(run_root).expect("run root");
    let path = run_root.join("m80-net-helper-test");
    let mut file = std::fs::File::create(&path).expect("fake net helper");
    file.write_all(
        br#"#!/bin/sh
while IFS= read -r _line; do
  printf '%s\n' '{"status":"ok","success":{"kind":"empty"}}'
done
"#,
    )
    .expect("write fake net helper");
    file.sync_all().expect("sync fake net helper");
    drop(file);
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    path
}
