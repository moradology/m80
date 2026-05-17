//! Phase F composed real-KVM driver for layered-rootfs warm pools.
//!
//! This ignored test is owned by `m80-q420k.6.1`. It emits the three JSON
//! artifacts consumed by the Phase F measurement beads; those measurement beads
//! remain responsible for verified numeric close discipline.

mod common;
#[path = "e2e_composed_real_kvm/memory.rs"]
mod memory;
mod pmem_shared_support;
#[path = "e2e_composed_real_kvm/quiet_host.rs"]
mod quiet_host;
mod snapshot_template_support;

use std::collections::BTreeSet;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Barrier};
use std::thread;

use m80_firecracker::{
    load_boot_spec_yaml_str, Backend, BackendConfig, BootSpec, BootSpecWarmStrategy, CgroupMode,
    SandboxConfig, TemplateFingerprint, TemplateStore, WarmLease, WarmPool, WarmPoolConfig,
    WarmPoolSnapshot, WarmStrategy,
};
use m80_image_store::{ImageKind, ImageStore, DEFAULT_STORE_ROOT};
use m80_proto::{ExecRequest, ExecStatus};
use serde_json::json;

const SHARED_DIGEST_PLACEHOLDER: &str =
    "1111111111111111111111111111111111111111111111111111111111111111";
const PERVM_DIGEST_PLACEHOLDER: &str =
    "2222222222222222222222222222222222222222222222222222222222222222";
const TEMPLATE_FINGERPRINT_PLACEHOLDER: &str =
    "3333333333333333333333333333333333333333333333333333333333333333";
const TEMPLATE_STORE_PLACEHOLDER: &str = "/tmp/m80-composed-e2e-template-store";
const FIXTURE: &str = include_str!("fixtures/composed-e2e/bootspec.yaml");
const BACKGROUND_REFILL_HEADROOM: usize = 4;

#[test]
#[ignore = "requires real KVM, Firecracker, jailer, pmem, erofs+DAX, and snapshot templates"]
fn composed_e2e_layered_warm_pool() {
    let _guard = snapshot_template_support::real_kvm_test_lock();
    let (allow_other_firecracker_vms, preexisting_firecrackers) =
        quiet_host::assert_or_record_quiet_host("M80_COMPOSED_E2E_ALLOW_OTHER_VMS");
    let mut substrate =
        quiet_host::substrate_json(allow_other_firecracker_vms, &preexisting_firecrackers);
    let target_ready = env_usize("M80_COMPOSED_E2E_N", 10);
    assert!(target_ready > 0, "M80_COMPOSED_E2E_N must be positive");
    let shared_payload_mib = env_usize("M80_COMPOSED_E2E_SHARED_PAYLOAD_MIB", 32);
    let artifact_dir = artifact_dir();
    let restore_artifact_path = artifact_dir.join("composed-e2e-restore-N10.json");
    let memory_artifact_path = artifact_dir.join("composed-e2e-host-memory.json");
    let residue_artifact_path = artifact_dir.join("composed-e2e-residue.json");
    let git_worktree_dirty_excluding_artifacts = quiet_host::git_worktree_dirty_excluding(&[
        restore_artifact_path.clone(),
        memory_artifact_path.clone(),
        residue_artifact_path.clone(),
    ]);
    let git_commit = quiet_host::git_head_commit();

    let discovery = snapshot_template_support::real_kvm_discovery();
    quiet_host::record_preflight_artifacts(&mut substrate, &discovery);
    let store = open_default_image_store();
    let shared_digest = pmem_shared_support::build_payload_image_digest(&store, shared_payload_mib);
    let per_vm_digest = pmem_shared_support::build_test_image_digest(&store, 1);
    store
        .verify(&shared_digest)
        .expect("verify Shared erofs image");
    store
        .verify(&per_vm_digest)
        .expect("verify PerVm erofs image");
    let shared_image_path = pmem_shared_support::store_erofs_path(&store, &shared_digest);
    let shared_payload_layout = pmem_shared_support::assert_payload_file_uncompressed_non_inlined(
        &shared_image_path,
        "payload.bin",
    );
    let shared_image_metadata =
        std::fs::metadata(&shared_image_path).expect("shared image metadata");
    let shared_image_bytes = shared_image_metadata.len();

    let prefix = snapshot_template_support::unique_suffix("composed");
    let admission_capacity = target_ready * 2 + BACKGROUND_REFILL_HEADROOM;
    let backend = Arc::new(
        Backend::new(make_backend_config(
            discovery.clone(),
            admission_capacity as u32,
        ))
        .expect("Backend::new composed"),
    );
    let template_temp = tempfile::Builder::new()
        .prefix("composed-e2e-templates-")
        .tempdir_in(&discovery.run_root)
        .expect("template parent under run root");
    let template_store_root = template_temp.path().join("templates");

    let pre_fingerprint_text =
        boot_spec_text(&shared_digest, &per_vm_digest, &template_store_root, None);
    let pre_spec = load_boot_spec_yaml_str(&pre_fingerprint_text).expect("parse pre-fingerprint");
    let per_vm_baseline = memory::measure_per_vm_baseline(
        Arc::clone(&backend),
        &discovery.run_root,
        &template_temp.path().join("pervm-baseline-templates"),
        &pre_spec,
        &store,
        &shared_digest,
        shared_image_bytes,
        &per_vm_digest,
        target_ready,
        admission_capacity,
        &format!("{prefix}-pvb"),
    );
    let residue_before = ResidueBaseline::capture(&discovery.run_root);
    let hooks = snapshot_hooks(&pre_spec);
    let pre_sandbox = sandbox_config_for_boot_spec(&pre_spec, format!("{prefix}-template-inputs"));
    let inputs = backend
        .snapshot_template_inputs(&pre_sandbox, hooks.clone())
        .expect("compute composed template inputs");
    let fingerprint = TemplateFingerprint::compute(&inputs);

    let spec_text = boot_spec_text(
        &shared_digest,
        &per_vm_digest,
        &template_store_root,
        Some(&fingerprint),
    );
    let spec = load_boot_spec_yaml_str(&spec_text).expect("parse composed BootSpec fixture");
    assert_boot_spec_matches_runtime(&spec, &template_store_root, &fingerprint);
    let sandbox = sandbox_config_for_boot_spec(&spec, format!("{prefix}-template"));
    let hooks = snapshot_hooks(&spec);
    let ready_probe = snapshot_ready_probe(&spec);
    let template_store = Arc::new(
        TemplateStore::create(template_store_root.clone(), admission_capacity)
            .expect("create template store"),
    );
    let warmup_pool = WarmPool::new(
        Arc::clone(&backend),
        WarmPoolConfig {
            target_ready: 1,
            sandbox: sandbox.clone(),
            strategy: WarmStrategy::snapshot_restore(
                Arc::clone(&template_store),
                hooks.clone(),
                ready_probe.clone(),
            ),
            vm_id_prefix: format!("{prefix}-warmup"),
            cpu_allocator: None,
        },
    )
    .expect("WarmPool::new composed warmup");
    warmup_pool
        .fill_to_target_blocking()
        .expect("build composed snapshot template");
    let template_build_samples_us = warmup_pool.take_fill_duration_samples_us();
    assert_eq!(template_build_samples_us.len(), 1);
    drop(warmup_pool);
    snapshot_template_support::assert_no_run_dirs_with_prefix(
        &discovery.run_root,
        &format!("{prefix}-warmup"),
    );
    let pool = WarmPool::new(
        Arc::clone(&backend),
        WarmPoolConfig {
            target_ready,
            sandbox,
            strategy: WarmStrategy::snapshot_restore(
                Arc::clone(&template_store),
                hooks,
                ready_probe,
            ),
            vm_id_prefix: prefix.clone(),
            cpu_allocator: Some(memory::no_refill_allocator(target_ready)),
        },
    )
    .expect("WarmPool::new composed");

    let baseline_before_fill_bytes = mem_available_bytes();
    pool.fill_to_target_blocking()
        .expect("fill composed snapshot-template pool");
    let after_fill_bytes = mem_available_bytes();
    let fill_snapshot = pool.snapshot();
    assert_eq!(fill_snapshot.ready, target_ready);
    assert_eq!(fill_snapshot.fill_failures_total, 0);
    let fill_samples_us = pool.take_fill_duration_samples_us();
    assert!(
        fill_samples_us.len() >= target_ready,
        "expected at least {target_ready} fill samples, got {}",
        fill_samples_us.len()
    );

    let pool = Arc::new(pool);
    let start = Arc::new(Barrier::new(target_ready + 1));
    let hold = Arc::new(Barrier::new(target_ready + 1));
    let (tx, rx) = mpsc::channel();
    let mut handles = Vec::with_capacity(target_ready);
    for worker in 0..target_ready {
        let pool = Arc::clone(&pool);
        let start = Arc::clone(&start);
        let hold = Arc::clone(&hold);
        let tx = tx.clone();
        handles.push(thread::spawn(move || {
            let mut lease = pool.try_lease().expect("lease composed slot");
            let run_dir = lease.run_dir().to_path_buf();
            start.wait();
            let result = exercise_composed_lease(&mut lease, worker);
            tx.send((run_dir, result)).expect("send composed result");
            hold.wait();
            lease.discard().expect("discard composed lease");
        }));
    }
    drop(tx);
    start.wait();

    let mut lease_results = Vec::with_capacity(target_ready);
    let mut leased_run_dirs = Vec::with_capacity(target_ready);
    for _ in 0..target_ready {
        let (run_dir, result) = rx.recv().expect("receive composed worker result");
        leased_run_dirs.push(run_dir);
        lease_results.push(result);
    }
    assert_eq!(lease_results.len(), target_ready);
    let diagnostics_summaries = collect_diagnostics_summaries(&leased_run_dirs);
    assert_composed_diagnostics(&diagnostics_summaries, target_ready);
    let during_leases_snapshot = pool.snapshot();
    memory::assert_attached_without_refill(during_leases_snapshot, target_ready);
    let after_attached_bytes = mem_available_bytes();
    hold.wait();
    for handle in handles {
        handle.join().expect("composed lease thread");
    }
    let after_teardown_bytes = mem_available_bytes();
    let after_discard_snapshot = pool.snapshot();
    drop(pool);
    snapshot_template_support::assert_no_run_dirs_with_prefix(&discovery.run_root, &prefix);

    assert_unique_lease_results(&lease_results, target_ready);
    assert_eq!(
        store.shared_ref_count(&shared_digest).unwrap(),
        0,
        "all Shared refs must be released after composed teardown"
    );
    assert_eq!(
        store
            .sweep_shared_refs(std::iter::empty::<&str>())
            .expect("post-composed shared-ref sweep"),
        0,
        "composed teardown must leave no stale Shared markers"
    );

    let after_attached_delta_bytes = memory::checked_mem_available_delta(
        baseline_before_fill_bytes,
        after_attached_bytes,
        "composed Shared attached checkpoint",
    );
    let bound_bytes =
        shared_image_bytes + per_vm_baseline.per_vm_overhead_bytes * target_ready as u64;
    assert!(
        after_attached_delta_bytes <= bound_bytes,
        "composed host memory delta {after_attached_delta_bytes} exceeded bound {bound_bytes}"
    );

    let residue = ResidueReport::capture(
        &residue_before,
        &discovery.run_root,
        &leased_run_dirs,
        &store,
        &[shared_digest.as_str(), per_vm_digest.as_str()],
        &template_store_root,
    );
    assert!(
        residue.unexpected_paths.is_empty(),
        "unexpected composed residue: {:?}",
        residue.unexpected_paths
    );
    let post_run_firecrackers = quiet_host::firecracker_processes();
    quiet_host::record_post_run_firecracker_processes(&mut substrate, &post_run_firecrackers);

    write_json_artifact(
        &restore_artifact_path,
        restore_artifact(
            target_ready,
            &template_build_samples_us,
            &fill_samples_us,
            fill_snapshot,
            during_leases_snapshot,
            after_discard_snapshot,
            &diagnostics_summaries,
            substrate.clone(),
            git_worktree_dirty_excluding_artifacts,
            &git_commit,
        ),
    );
    write_json_artifact(
        &memory_artifact_path,
        json!({
            "schema_version": 1,
            "scenario": "composed_e2e_layered_warm_pool",
            "substrate": substrate.clone(),
            "git_worktree_dirty_excluding_artifacts": git_worktree_dirty_excluding_artifacts,
            "git_commit": git_commit.as_str(),
            "page_cache_dropped_between_fill_and_attach": false,
            "data": {
                "host_memory": {
                    "n_attached": target_ready,
                    "baseline_before_fill_bytes": baseline_before_fill_bytes,
                    "after_fill_bytes": after_fill_bytes,
                    "after_n_attached_bytes": after_attached_bytes,
                    "after_teardown_bytes": after_teardown_bytes,
                    "after_n_attached_delta_bytes": after_attached_delta_bytes,
                    "shared_image_bytes": shared_image_bytes,
                    "per_vm_overhead_bytes": per_vm_baseline.per_vm_overhead_bytes,
                    "per_vm_overhead_source": "per_vm_baseline_same_run",
                    "bound_bytes": bound_bytes,
                    "bound_satisfied": after_attached_delta_bytes <= bound_bytes,
                    "shared_image_digest": shared_digest.as_str(),
                    "shared_image_path": shared_image_path.display().to_string(),
                    "shared_image_dev": shared_image_metadata.dev(),
                    "shared_image_ino": shared_image_metadata.ino(),
                    "shared_payload_layout": {
                        "path": "payload.bin",
                        "erofs_layout": shared_payload_layout.layout,
                        "size_bytes": shared_payload_layout.size_bytes,
                        "on_disk_size_bytes": shared_payload_layout.on_disk_size_bytes,
                        "compression_ratio": shared_payload_layout.compression_ratio,
                        "raw_dump": shared_payload_layout.raw_dump,
                    },
                    "per_vm_baseline": per_vm_baseline.to_json(),
                }
            }
        }),
    );
    write_json_artifact(
        &residue_artifact_path,
        residue.to_json(
            target_ready,
            &shared_digest,
            &per_vm_digest,
            &fingerprint,
            substrate,
            git_worktree_dirty_excluding_artifacts,
            &git_commit,
        ),
    );

    println!(
        "M80_COMPOSED_E2E artifacts_dir={} n={} shared_digest={} per_vm_digest={} fingerprint={}",
        artifact_dir.display(),
        target_ready,
        shared_digest.as_str(),
        per_vm_digest.as_str(),
        fingerprint.to_hex()
    );
}

#[derive(Debug)]
struct LeaseResult {
    vm_id: String,
    machine_id: String,
    hostname: String,
    shared_mount: String,
    per_vm_mount: String,
    shared_payload: String,
    per_vm_payload: String,
    scratch: String,
}

fn exercise_composed_lease(lease: &mut WarmLease, worker: usize) -> LeaseResult {
    let vm_id = lease.vm_id().to_owned();
    let machine_id = exec_sh(lease, "cat /etc/machine-id", "read machine-id")
        .trim()
        .to_owned();
    let hostname = exec_sh(
        lease,
        "cat /proc/sys/kernel/hostname",
        "read kernel hostname",
    )
    .trim()
    .to_owned();
    let mounts = exec_sh(lease, "cat /proc/mounts", "read mounts");
    let shared_mount = find_pmem_mount(&mounts, 0);
    let per_vm_mount = find_pmem_mount(&mounts, 1);
    let shared_payload = exec_sh(
        lease,
        "dd if=/opt/m80-layers/smoke-0/payload.bin of=/dev/null bs=4M status=none && cat /opt/m80-layers/smoke-0/payload.txt",
        "read Shared payload",
    )
    .trim()
    .to_owned();
    let per_vm_payload = exec_sh(
        lease,
        "cat /opt/m80-layers/smoke-1/payload.txt",
        "read PerVm payload",
    )
    .trim()
    .to_owned();
    let scratch = exec_sh(
        lease,
        &format!("echo worker-{worker} >/tmp/m80-composed-worker && cat /tmp/m80-composed-worker"),
        "write per-lease scratch marker",
    )
    .trim()
    .to_owned();

    LeaseResult {
        vm_id,
        machine_id,
        hostname,
        shared_mount,
        per_vm_mount,
        shared_payload,
        per_vm_payload,
        scratch,
    }
}

fn assert_unique_lease_results(results: &[LeaseResult], expected: usize) {
    let mut vm_ids = BTreeSet::new();
    let mut machine_ids = BTreeSet::new();
    let mut scratches = BTreeSet::new();
    for result in results {
        snapshot_template_support::assert_machine_id(&result.machine_id);
        assert_eq!(result.hostname, "m80-composed");
        assert!(
            result.shared_mount.contains("dax"),
            "Shared mount must advertise DAX: {}",
            result.shared_mount
        );
        assert!(
            result.per_vm_mount.contains("dax"),
            "PerVm mount must advertise DAX: {}",
            result.per_vm_mount
        );
        assert!(
            result.shared_payload.contains("pmem density payload"),
            "unexpected Shared payload marker: {:?}",
            result.shared_payload
        );
        assert!(
            result.per_vm_payload.contains("pmem payload slot 1"),
            "unexpected PerVm payload marker: {:?}",
            result.per_vm_payload
        );
        vm_ids.insert(result.vm_id.clone());
        machine_ids.insert(result.machine_id.clone());
        scratches.insert(result.scratch.clone());
    }
    assert_eq!(vm_ids.len(), expected);
    assert_eq!(machine_ids.len(), expected);
    assert_eq!(
        scratches,
        (0..expected)
            .map(|worker| format!("worker-{worker}"))
            .collect::<BTreeSet<_>>()
    );
}

fn find_pmem_mount(mounts: &str, slot: usize) -> String {
    let mount_path = format!("/opt/m80-layers/smoke-{slot}");
    let device = format!("/dev/pmem{slot}");
    let line = mounts
        .lines()
        .find(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            fields.first() == Some(&device.as_str()) && fields.get(1) == Some(&mount_path.as_str())
        })
        .unwrap_or_else(|| panic!("missing mount {mount_path}; all mounts:\n{mounts}"));
    let fields = line.split_whitespace().collect::<Vec<_>>();
    assert_eq!(fields.get(2).copied(), Some("erofs"), "{line}");
    let options = fields.get(3).expect("mount options");
    assert!(
        options.split(',').any(|option| option == "ro"),
        "pmem mount must be read-only: {line}"
    );
    assert!(
        options
            .split(',')
            .any(|option| option == "dax" || option.starts_with("dax=")),
        "pmem mount must advertise DAX: {line}"
    );
    line.to_owned()
}

fn exec_sh(lease: &mut WarmLease, script: &str, label: &str) -> String {
    let response = lease
        .exec(ExecRequest {
            program: "/bin/sh".to_owned(),
            args: vec!["-c".to_owned(), script.to_owned()],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(30_000),
            streaming: false,
        })
        .unwrap_or_else(|err| panic!("{label}: {err}"));
    assert!(
        response.status == ExecStatus::Completed && response.exit_code == Some(0),
        "{label} failed: status={:?} exit={:?} stdout={} stderr={}",
        response.status,
        response.exit_code,
        String::from_utf8_lossy(&response.stdout),
        String::from_utf8_lossy(&response.stderr)
    );
    String::from_utf8(response.stdout).unwrap_or_else(|err| panic!("{label}: stdout utf8: {err}"))
}

#[derive(Debug)]
struct DiagnosticsSummary {
    run_dir: String,
    vm_id: String,
    phase_completed: BTreeSet<String>,
    lifecycle_messages: BTreeSet<String>,
    exec_completed_count: usize,
}

fn collect_diagnostics_summaries(run_dirs: &[PathBuf]) -> Vec<DiagnosticsSummary> {
    run_dirs
        .iter()
        .map(|run_dir| {
            let path = run_dir.join(m80_observability::DIAGNOSTICS_FILE_NAME);
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
            let events = text
                .lines()
                .map(|line| {
                    serde_json::from_str::<serde_json::Value>(line)
                        .unwrap_or_else(|err| panic!("parse diagnostics line {line:?}: {err}"))
                })
                .collect::<Vec<_>>();
            let vm_id = events
                .iter()
                .find_map(|event| event["context"]["vm_id"].as_str())
                .unwrap_or("<missing-vm-id>")
                .to_owned();
            let phase_completed = events
                .iter()
                .filter(|event| event["event_kind"] == "phase_completed")
                .filter_map(|event| event["context"]["phase_name"].as_str())
                .map(ToOwned::to_owned)
                .collect::<BTreeSet<_>>();
            let lifecycle_messages = events
                .iter()
                .filter(|event| event["event_kind"] == "lifecycle")
                .filter_map(|event| event["message"].as_str())
                .map(ToOwned::to_owned)
                .collect::<BTreeSet<_>>();
            let exec_completed_count = events
                .iter()
                .filter(|event| {
                    event["event_kind"] == "lifecycle"
                        && event["message"] == "exec request completed"
                })
                .count();
            DiagnosticsSummary {
                run_dir: run_dir.display().to_string(),
                vm_id,
                phase_completed,
                lifecycle_messages,
                exec_completed_count,
            }
        })
        .collect()
}

fn assert_composed_diagnostics(summaries: &[DiagnosticsSummary], expected: usize) {
    assert_eq!(summaries.len(), expected);
    for summary in summaries {
        for phase_name in [
            "phase_3_storage_prep",
            "phase_restore_load",
            "phase_restore_probe_exec_channel",
            "phase_restore_post_restore_hooks",
        ] {
            assert!(
                summary.phase_completed.contains(phase_name),
                "{} missing diagnostics phase {phase_name}: {:?}",
                summary.run_dir,
                summary.phase_completed
            );
        }
        for message in ["snapshot restored", "restored guestd ready"] {
            assert!(
                summary.lifecycle_messages.contains(message),
                "{} missing diagnostics lifecycle message {message}: {:?}",
                summary.run_dir,
                summary.lifecycle_messages
            );
        }
        assert!(
            summary.exec_completed_count >= 5,
            "{} expected workload exec completions, got {}",
            summary.run_dir,
            summary.exec_completed_count
        );
    }
}

fn open_default_image_store() -> ImageStore {
    std::fs::create_dir_all(DEFAULT_STORE_ROOT).expect("create default image store root");
    ImageStore::open_default().expect("open default image store")
}

fn boot_spec_text(
    shared_digest: &m80_image_store::ImageDigest,
    per_vm_digest: &m80_image_store::ImageDigest,
    template_store_root: &Path,
    fingerprint: Option<&TemplateFingerprint>,
) -> String {
    FIXTURE
        .replace(SHARED_DIGEST_PLACEHOLDER, shared_digest.as_str())
        .replace(PERVM_DIGEST_PLACEHOLDER, per_vm_digest.as_str())
        .replace(
            TEMPLATE_FINGERPRINT_PLACEHOLDER,
            &fingerprint
                .map(TemplateFingerprint::to_hex)
                .unwrap_or_else(|| TEMPLATE_FINGERPRINT_PLACEHOLDER.to_owned()),
        )
        .replace(
            TEMPLATE_STORE_PLACEHOLDER,
            &template_store_root.display().to_string(),
        )
}

fn assert_boot_spec_matches_runtime(
    spec: &BootSpec,
    template_store_root: &Path,
    fingerprint: &TemplateFingerprint,
) {
    assert_eq!(spec.name.as_deref(), Some("composed-e2e"));
    assert_eq!(spec.pmem_layers.len(), 2);
    let BootSpecWarmStrategy::SnapshotRestore {
        template_store,
        template_fingerprint,
        ready_probe,
        hooks,
    } = &spec.warm_strategy
    else {
        panic!("composed fixture must request snapshot_restore");
    };
    assert_eq!(template_store, template_store_root);
    assert_eq!(template_fingerprint, fingerprint);
    assert_eq!(ready_probe.program, "/bin/true");
    assert_eq!(hooks.hooks().len(), 3);
}

fn snapshot_hooks(spec: &BootSpec) -> m80_firecracker::HookSpecSet {
    let BootSpecWarmStrategy::SnapshotRestore { hooks, .. } = &spec.warm_strategy else {
        panic!("composed fixture must request snapshot_restore");
    };
    hooks.clone()
}

fn snapshot_ready_probe(spec: &BootSpec) -> ExecRequest {
    let BootSpecWarmStrategy::SnapshotRestore { ready_probe, .. } = &spec.warm_strategy else {
        panic!("composed fixture must request snapshot_restore");
    };
    ExecRequest {
        program: ready_probe.program.clone(),
        args: ready_probe.args.clone(),
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(ready_probe.timeout_ms),
        streaming: false,
    }
}

fn sandbox_config_for_boot_spec(spec: &BootSpec, request_id: String) -> SandboxConfig {
    SandboxConfig {
        vm_id: None,
        workspace: spec.sandbox.workspace.clone(),
        network: spec.sandbox.network.clone(),
        vcpu_count: Some(spec.sandbox.vcpu_count),
        mem_size_mib: Some(spec.sandbox.mem_size_mib),
        boot_args: (!spec.sandbox.boot_args.is_empty()).then(|| spec.sandbox.boot_args.join(" ")),
        overlay_size_bytes: spec.sandbox.overlay_size_bytes,
        request_id: Some(request_id),
        pmem_layers: spec.pmem_layers.clone(),
        ..common::sandbox_config()
    }
}

fn make_backend_config(
    discovery: m80_preflight::Discovery,
    max_concurrent_vms: u32,
) -> BackendConfig {
    let run_root = discovery.run_root.clone();
    BackendConfig::builder(discovery)
        .max_concurrent_vms(max_concurrent_vms)
        .run_root(run_root)
        .jail_uid(jail_id_from_env("M80_JAIL_UID", 3000))
        .jail_gid(jail_id_from_env("M80_JAIL_GID", 3000))
        .cgroup_mode(CgroupMode::Disabled)
        .build()
}

fn jail_id_from_env(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(default)
}

fn restore_artifact(
    target_ready: usize,
    template_build_samples_us: &[u64],
    samples_us: &[u64],
    fill_snapshot: WarmPoolSnapshot,
    during_leases_snapshot: WarmPoolSnapshot,
    after_discard_snapshot: WarmPoolSnapshot,
    diagnostics_summaries: &[DiagnosticsSummary],
    substrate: serde_json::Value,
    git_worktree_dirty_excluding_artifacts: bool,
    git_commit: &str,
) -> serde_json::Value {
    let samples_ms = samples_us
        .iter()
        .map(|sample| *sample as f64 / 1000.0)
        .collect::<Vec<_>>();
    json!({
        "schema_version": 1,
        "scenario": "composed_e2e_layered_warm_pool",
        "substrate": substrate,
        "git_worktree_dirty_excluding_artifacts": git_worktree_dirty_excluding_artifacts,
        "git_commit": git_commit,
        "page_cache_dropped_between_leases": false,
        "data": {
            "restore_latency": {
                "count": samples_ms.len(),
                "target_ready": target_ready,
                "samples_ms": samples_ms,
                "p50_ms": percentile_ms(samples_us, 50),
                "p95_ms": percentile_ms(samples_us, 95),
                "p99_ms": percentile_ms(samples_us, 99),
                "fail_count": fill_snapshot.fill_failures_total,
                "template_build_warmup_ms": template_build_samples_us
                    .iter()
                    .map(|sample| *sample as f64 / 1000.0)
                    .collect::<Vec<_>>(),
            },
            "warm_pool": {
                "after_fill": snapshot_json(fill_snapshot),
                "during_leases": snapshot_json(during_leases_snapshot),
                "after_discard": snapshot_json(after_discard_snapshot),
            },
            "observability": {
                "diagnostics": diagnostics_summaries
                    .iter()
                    .map(diagnostics_summary_json)
                    .collect::<Vec<_>>(),
                "pmem_layers_by_sharing": {
                    "Shared": 1,
                    "PerVm": 1,
                }
            }
        }
    })
}

fn diagnostics_summary_json(summary: &DiagnosticsSummary) -> serde_json::Value {
    json!({
        "run_dir": summary.run_dir,
        "vm_id": summary.vm_id,
        "phase_completed": summary.phase_completed,
        "lifecycle_messages": summary.lifecycle_messages,
        "exec_completed_count": summary.exec_completed_count,
    })
}

fn snapshot_json(snapshot: WarmPoolSnapshot) -> serde_json::Value {
    json!({
        "target_ready": snapshot.target_ready,
        "ready": snapshot.ready,
        "filling": snapshot.filling,
        "leased": snapshot.leased,
        "discarded": snapshot.discarded,
        "consecutive_fill_errors": snapshot.consecutive_fill_errors,
        "fill_attempts_total": snapshot.fill_attempts_total,
        "fill_failures_total": snapshot.fill_failures_total,
        "lease_acquired_total": snapshot.lease_acquired_total,
        "lease_returned_total": snapshot.lease_returned_total,
    })
}

fn percentile_ms(samples_us: &[u64], percentile: usize) -> f64 {
    if samples_us.is_empty() {
        return 0.0;
    }
    let mut samples = samples_us.to_vec();
    samples.sort_unstable();
    let index = ((samples.len() * percentile).div_ceil(100)).saturating_sub(1);
    samples[index] as f64 / 1000.0
}

fn artifact_dir() -> PathBuf {
    std::env::var_os("M80_COMPOSED_E2E_OUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("benches")
                .join("snapshots")
        })
}

fn write_json_artifact(path: &Path, value: serde_json::Value) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create composed artifact parent");
    }
    let mut body = serde_json::to_string_pretty(&value).expect("artifact json");
    body.push('\n');
    std::fs::write(path, body).unwrap_or_else(|err| panic!("write {}: {err}", path.display()));
    println!("M80_COMPOSED_E2E_ARTIFACT {}", path.display());
}

fn mem_available_bytes() -> u64 {
    let meminfo = std::fs::read_to_string("/proc/meminfo").expect("read /proc/meminfo");
    for line in meminfo.lines() {
        if let Some(rest) = line.strip_prefix("MemAvailable:") {
            let kib = rest
                .split_whitespace()
                .next()
                .expect("MemAvailable value")
                .parse::<u64>()
                .expect("MemAvailable value is integer KiB");
            return kib * 1024;
        }
    }
    panic!("MemAvailable not found in /proc/meminfo");
}

fn env_usize(name: &str, default: usize) -> usize {
    match std::env::var(name) {
        Ok(raw) => raw
            .parse::<usize>()
            .unwrap_or_else(|_| panic!("{name} must be a positive integer, got {raw:?}")),
        Err(_) => default,
    }
}

#[derive(Debug)]
struct ResidueBaseline {
    run_root_entries: BTreeSet<PathBuf>,
    tmp_m80_entries: BTreeSet<PathBuf>,
    var_run_m80_entries: BTreeSet<PathBuf>,
}

impl ResidueBaseline {
    fn capture(run_root: &Path) -> Self {
        Self {
            run_root_entries: shallow_entries(run_root),
            tmp_m80_entries: tmp_m80_entries(),
            var_run_m80_entries: shallow_entries(Path::new("/var/run/m80")),
        }
    }
}

#[derive(Debug)]
struct ResidueReport {
    scanned_roots: Vec<String>,
    unexpected_paths: Vec<String>,
    image_store_preserved: Vec<String>,
    image_store_expected: Vec<String>,
    template_store_preserved: Vec<String>,
    leased_run_dirs: Vec<String>,
}

impl ResidueReport {
    #[allow(clippy::too_many_arguments)]
    fn capture(
        before: &ResidueBaseline,
        run_root: &Path,
        leased_run_dirs: &[PathBuf],
        image_store: &ImageStore,
        expected_image_digests: &[&str],
        template_store_root: &Path,
    ) -> Self {
        let mut unexpected_paths = Vec::new();
        unexpected_paths.extend(
            shallow_entries(run_root)
                .difference(&before.run_root_entries)
                .map(display_path),
        );
        unexpected_paths.extend(
            tmp_m80_entries()
                .difference(&before.tmp_m80_entries)
                .map(display_path),
        );
        unexpected_paths.extend(
            shallow_entries(Path::new("/var/run/m80"))
                .difference(&before.var_run_m80_entries)
                .map(display_path),
        );
        for run_dir in leased_run_dirs {
            if run_dir.exists() {
                unexpected_paths.push(display_path(run_dir));
            }
        }

        let image_store_expected = expected_image_digests
            .iter()
            .map(|digest| (*digest).to_owned())
            .collect::<Vec<_>>();
        let mut image_store_preserved = Vec::with_capacity(expected_image_digests.len());
        for digest in expected_image_digests {
            let digest = m80_image_store::ImageDigest::parse(digest).expect("expected digest");
            let artifact = image_store
                .resolve_as(&digest, ImageKind::Erofs)
                .expect("expected composed image artifact still present");
            image_store_preserved.push(artifact.digest().as_str().to_owned());
        }
        let template_store_preserved =
            snapshot_template_support::index_fingerprints(template_store_root)
                .into_iter()
                .map(|fingerprint| fingerprint.to_hex())
                .collect::<Vec<_>>();
        Self {
            scanned_roots: vec![
                run_root.display().to_string(),
                "/tmp/m80-*".to_owned(),
                "/var/run/m80".to_owned(),
                DEFAULT_STORE_ROOT.to_owned(),
                template_store_root.display().to_string(),
            ],
            unexpected_paths,
            image_store_preserved,
            image_store_expected,
            template_store_preserved,
            leased_run_dirs: leased_run_dirs.iter().map(display_path).collect(),
        }
    }

    fn to_json(
        &self,
        target_ready: usize,
        shared_digest: &m80_image_store::ImageDigest,
        per_vm_digest: &m80_image_store::ImageDigest,
        fingerprint: &TemplateFingerprint,
        substrate: serde_json::Value,
        git_worktree_dirty_excluding_artifacts: bool,
        git_commit: &str,
    ) -> serde_json::Value {
        json!({
            "schema_version": 1,
            "scenario": "composed_e2e_layered_warm_pool",
            "substrate": substrate,
            "git_worktree_dirty_excluding_artifacts": git_worktree_dirty_excluding_artifacts,
            "git_commit": git_commit,
            "data": {
                "residue": {
                    "n_leases": target_ready,
                    "unexpected_paths": self.unexpected_paths,
                    "scanned_roots": self.scanned_roots,
                    "leased_run_dirs": self.leased_run_dirs,
                    "image_store": {
                        "preserved": self.image_store_preserved,
                        "expected": self.image_store_expected,
                        "shared_digest": shared_digest.as_str(),
                        "per_vm_digest": per_vm_digest.as_str(),
                    },
                    "template_store": {
                        "preserved": self.template_store_preserved,
                        "expected_fingerprint": fingerprint.to_hex(),
                    }
                }
            }
        })
    }
}

fn shallow_entries(root: &Path) -> BTreeSet<PathBuf> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return BTreeSet::new(),
        Err(err) => panic!("read {}: {err}", root.display()),
    };
    entries
        .map(|entry| entry.unwrap_or_else(|err| panic!("read {} entry: {err}", root.display())))
        .map(|entry| entry.path())
        .collect()
}

fn tmp_m80_entries() -> BTreeSet<PathBuf> {
    shallow_entries(Path::new("/tmp"))
        .into_iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("m80-"))
        })
        .collect()
}

fn display_path(path: &PathBuf) -> String {
    path.display().to_string()
}
