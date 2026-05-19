//! Real-KVM composition coverage for snapshot-template warm pools.

mod common;
mod pmem_shared_support;
mod snapshot_template_support;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use m80_firecracker::{HookSpec, HookSpecSet, WarmLease};
use serde_json::json;
use snapshot_template_support::*;
use tempfile::TempDir;

#[test]
#[ignore = "requires KVM host with real Firecracker binary and snapshot-template support"]
fn e2e_2_restore_refill_scaffold() {
    let _guard = real_kvm_test_lock();
    let discovery = real_kvm_discovery();
    let temp = TempDir::new().expect("template store");
    let suffix = unique_suffix("tpllat");
    let pool = template_pool(
        &discovery,
        temp.path(),
        HookSpecSet::new(vec![HookSpec::RegenMachineId]),
        sandbox_config(format!("{suffix}-base"), None),
        suffix,
    );
    pool.fill_to_target_blocking()
        .expect("initial cached template fill");

    let mut samples_us = Vec::new();
    for _ in 0..20 {
        let lease = pool.try_lease().expect("lease latency slot");
        let started = Instant::now();
        lease.discard().expect("discard latency slot");
        pool.wait_for_ready(1, Duration::from_secs(120))
            .expect("latency refill");
        samples_us.push(started.elapsed().as_micros() as u64);
    }
    let p99_us = percentile_99(samples_us.clone());
    let artifact = latency_artifact_path(&discovery);
    write_latency_artifact(&artifact, &samples_us, p99_us);
    println!(
        "snapshot-template refill scaffold artifact: {}",
        artifact.display()
    );
    assert_eq!(samples_us.len(), 20);
    assert!(p99_us > 0, "refill samples must record positive durations");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary and snapshot-template support"]
fn e2e_7_template_invalidation() {
    let _guard = real_kvm_test_lock();
    let discovery = real_kvm_discovery();
    let temp = TempDir::new().expect("template store");
    let store_root = temp.path().join("templates");
    let suffix = unique_suffix("tplinv");

    let first_pool = template_pool(
        &discovery,
        temp.path(),
        HookSpecSet::empty(),
        sandbox_config(format!("{suffix}-base"), None),
        format!("{suffix}a"),
    );
    first_pool.fill_to_target_blocking().expect("first fill");
    drop(first_pool);
    let first = index_fingerprints(&store_root);
    assert_eq!(first.len(), 1);

    let second_pool = template_pool(
        &discovery,
        temp.path(),
        HookSpecSet::empty(),
        sandbox_config(
            format!("{suffix}-base"),
            Some("m80.template_input_bump=1".to_owned()),
        ),
        format!("{suffix}b"),
    );
    second_pool
        .fill_to_target_blocking()
        .expect("changed-input fill");
    drop(second_pool);
    let second = index_fingerprints(&store_root);

    assert_eq!(second.len(), 2, "changed inputs must build a new template");
    assert!(
        second.contains(&first[0]),
        "changed-input fill must retain the original template entry"
    );
    let changed = second
        .iter()
        .filter(|fingerprint| **fingerprint != first[0])
        .count();
    assert_eq!(changed, 1, "changed inputs must add one new template");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary and snapshot-template support"]
fn e2e_8_concurrent_lease_isolation() {
    let _guard = real_kvm_test_lock();
    let discovery = real_kvm_discovery();
    let temp = TempDir::new().expect("template store");
    let suffix = unique_suffix("tpliso");
    let pool = Arc::new(template_pool_with_target(
        &discovery,
        temp.path(),
        HookSpecSet::new(vec![HookSpec::RegenMachineId]),
        sandbox_config(format!("{suffix}-base"), None),
        suffix,
        8,
        12,
    ));
    pool.fill_to_target_blocking()
        .expect("fill eight isolated slots");
    assert_eq!(pool.snapshot().ready, 8);

    let start = Arc::new(Barrier::new(9));
    let (tx, rx) = mpsc::channel();
    let mut handles = Vec::new();
    for worker in 0..8 {
        let pool = Arc::clone(&pool);
        let start = Arc::clone(&start);
        let tx = tx.clone();
        handles.push(thread::spawn(move || {
            start.wait();
            let mut lease = pool.try_lease().expect("concurrent lease");
            let vm_id = lease.vm_id().to_owned();
            let machine_id = exec_sh(&mut lease, "cat /etc/machine-id", "machine-id")
                .trim()
                .to_owned();
            let scratch = exec_sh(
                &mut lease,
                &format!("echo worker-{worker} >/tmp/m80-scratch && cat /tmp/m80-scratch"),
                "scratch marker",
            )
            .trim()
            .to_owned();
            lease.discard().expect("discard concurrent lease");
            tx.send((vm_id, machine_id, scratch))
                .expect("send lease result");
        }));
    }
    drop(tx);
    start.wait();

    let mut vm_ids = BTreeSet::new();
    let mut machine_ids = BTreeSet::new();
    let mut scratches = BTreeSet::new();
    for (vm_id, machine_id, scratch) in rx {
        assert_machine_id(&machine_id);
        vm_ids.insert(vm_id);
        machine_ids.insert(machine_id);
        scratches.insert(scratch);
    }
    for handle in handles {
        handle.join().expect("lease thread panicked");
    }

    assert_eq!(vm_ids.len(), 8);
    assert_eq!(machine_ids.len(), 8);
    assert_eq!(
        scratches,
        (0..8).map(|worker| format!("worker-{worker}")).collect()
    );
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary, pmem, and erofs+DAX support"]
fn e2e_9_compose_pmem_pervm() {
    let _guard = real_kvm_test_lock();
    let image_store = pmem_shared_support::open_default_store();
    let digest = pmem_shared_support::build_test_image_digest(&image_store, 0);
    let discovery = real_kvm_discovery();
    let temp = TempDir::new().expect("template store");
    let suffix = unique_suffix("tplpervm");
    let mut sandbox = sandbox_config(format!("{suffix}-base"), None);
    sandbox.pmem_layers = vec![pmem_shared_support::per_vm_layer(&digest, 0)];
    let pool = template_pool(
        &discovery,
        temp.path(),
        HookSpecSet::empty(),
        sandbox,
        suffix,
    );
    pool.fill_to_target_blocking()
        .expect("fill per-vm pmem template");

    let mut lease = pool.try_lease().expect("lease per-vm pmem slot");
    let mount = assert_pmem_mount(&mut lease, 0);
    assert!(
        lease.run_dir().join("pmem/0.img").is_file(),
        "PerVm restore must create a per-run backing clone"
    );
    lease.discard().expect("discard per-vm pmem lease");
    println!("per-vm snapshot-template pmem mount: {mount}");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary, pmem, and erofs+DAX support"]
fn e2e_10_compose_pmem_shared() {
    let _guard = real_kvm_test_lock();
    let image_store = pmem_shared_support::open_default_store();
    let digest = pmem_shared_support::build_test_image_digest(&image_store, 0);
    let store_path = pmem_shared_support::store_erofs_path(&image_store, &digest);
    let store_identity = pmem_shared_support::file_identity(&store_path);
    let discovery = real_kvm_discovery();
    let temp = TempDir::new().expect("template store");
    let suffix = unique_suffix("tplshared");
    let mut sandbox = sandbox_config(format!("{suffix}-base"), None);
    sandbox.pmem_layers = vec![pmem_shared_support::shared_layer(&digest, 0)];
    let pool = template_pool(
        &discovery,
        temp.path(),
        HookSpecSet::empty(),
        sandbox,
        suffix,
    );
    pool.fill_to_target_blocking()
        .expect("fill shared pmem template");

    let mut lease = pool.try_lease().expect("lease shared pmem slot");
    let mount = assert_pmem_mount(&mut lease, 0);
    let jail_backing =
        pmem_shared_support::jail_backing_path(lease.run_dir(), &discovery.firecracker_bin, 0);
    assert_eq!(
        pmem_shared_support::file_identity(&jail_backing),
        store_identity
    );
    assert!(
        !lease.run_dir().join("pmem/0.img").exists(),
        "Shared restore must not create a per-run backing clone"
    );
    assert_eq!(image_store.shared_ref_count(&digest).unwrap(), 1);
    lease.discard().expect("discard shared pmem lease");
    pool.wait_for_ready(1, Duration::from_secs(120))
        .expect("shared pmem refill");
    assert_eq!(
        image_store.shared_ref_count(&digest).unwrap(),
        1,
        "target-ready pool should hold one Shared ref after refill"
    );
    drop(pool);
    assert!(store_path.is_file(), "canonical shared artifact deleted");
    assert_eq!(image_store.shared_ref_count(&digest).unwrap(), 0);
    println!("shared snapshot-template pmem mount: {mount}");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary and snapshot-template support"]
fn e2e_11_no_leak_teardown() {
    let _guard = real_kvm_test_lock();
    let discovery = real_kvm_discovery();
    let temp = TempDir::new().expect("template store");
    let store_root = temp.path().join("templates");
    let suffix = unique_suffix("tplleak");
    let pool = template_pool(
        &discovery,
        temp.path(),
        HookSpecSet::new(vec![HookSpec::RegenMachineId]),
        sandbox_config(format!("{suffix}-base"), None),
        suffix.clone(),
    );
    pool.fill_to_target_blocking().expect("initial leak fill");
    for _ in 0..3 {
        let lease = pool.try_lease().expect("lease leak slot");
        lease.discard().expect("discard leak slot");
        pool.wait_for_ready(1, Duration::from_secs(120))
            .expect("refill leak slot");
    }
    drop(pool);

    assert_no_run_dirs_with_prefix(&discovery.run_root, &suffix);
    assert_eq!(index_fingerprints(&store_root).len(), 1);
    assert_empty_dir(&store_root.join("staging"));
}

fn assert_pmem_mount(lease: &mut WarmLease, slot: usize) -> String {
    let mounts = exec_sh(lease, "cat /proc/mounts", "read mounts");
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

fn percentile_99(mut samples: Vec<u64>) -> u64 {
    samples.sort_unstable();
    let index = ((samples.len() * 99).div_ceil(100)).saturating_sub(1);
    samples[index]
}

fn latency_artifact_path(discovery: &m80_preflight::Discovery) -> PathBuf {
    std::env::var_os("M80_SNAPSHOT_TEMPLATE_REFILL_JSON")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            discovery
                .run_root
                .join("snapshot-template-refill-scaffold.json")
        })
}

fn write_latency_artifact(path: &Path, samples_us: &[u64], p99_us: u64) {
    let payload = json!({
        "scenario": "e2e_2_restore_refill_scaffold",
        "metric": "discard_to_ready_refill_us",
        "measurement_status": "scaffold_only_verified_restore_to_handback_lives_in_m80_q420k_4_15",
        "unit": "microseconds",
        "samples": samples_us,
        "p99_us": p99_us,
    });
    let mut body = serde_json::to_string_pretty(&payload).expect("latency artifact json");
    body.push('\n');
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create latency artifact parent");
    }
    std::fs::write(path, body).expect("write latency artifact");
}

fn assert_empty_dir(path: &Path) {
    let mut entries = std::fs::read_dir(path)
        .unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
        .filter_map(Result::ok);
    assert!(entries.next().is_none(), "{} not empty", path.display());
}
