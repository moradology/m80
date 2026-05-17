use super::*;
use m80_firecracker::WarmPoolCpuAllocator;
use serde_json::json;

#[derive(Debug)]
pub(super) struct PerVmBaseline {
    pub(super) per_vm_overhead_bytes: u64,
    n_attached: usize,
    before_fill_bytes: u64,
    after_fill_bytes: u64,
    after_n_attached_bytes: u64,
    after_teardown_bytes: u64,
    after_n_attached_delta_bytes: u64,
    per_vm_payload_bytes: u64,
    image_digest: String,
    image_bytes: u64,
    shared_payload_digest: String,
    shared_payload_bytes: u64,
    template_fingerprint: String,
    attached_snapshot: WarmPoolSnapshot,
}

impl PerVmBaseline {
    pub(super) fn to_json(&self) -> serde_json::Value {
        json!({
            "n_attached": self.n_attached,
            "before_fill_bytes": self.before_fill_bytes,
            "after_fill_bytes": self.after_fill_bytes,
            "after_n_attached_bytes": self.after_n_attached_bytes,
            "after_teardown_bytes": self.after_teardown_bytes,
            "after_n_attached_delta_bytes": self.after_n_attached_delta_bytes,
            "per_vm_payload_bytes": self.per_vm_payload_bytes,
            "per_vm_overhead_bytes": self.per_vm_overhead_bytes,
            "image_digest": self.image_digest,
            "image_bytes": self.image_bytes,
            "shared_payload_digest": self.shared_payload_digest,
            "shared_payload_bytes": self.shared_payload_bytes,
            "template_fingerprint": self.template_fingerprint,
            "attached_snapshot": snapshot_json(self.attached_snapshot),
        })
    }
}

pub(super) fn no_refill_allocator(target_ready: usize) -> WarmPoolCpuAllocator {
    assert!(
        std::thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(1)
            >= target_ready,
        "host must expose at least {target_ready} CPUs for no-refill memory measurement"
    );
    WarmPoolCpuAllocator {
        first_cpu: 0,
        cpus_per_slot: 1,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn measure_per_vm_baseline(
    backend: Arc<Backend>,
    run_root: &Path,
    template_store_root: &Path,
    spec: &BootSpec,
    image_store: &ImageStore,
    shared_digest: &m80_image_store::ImageDigest,
    shared_image_bytes: u64,
    per_vm_digest: &m80_image_store::ImageDigest,
    target_ready: usize,
    admission_capacity: usize,
    prefix: &str,
) -> PerVmBaseline {
    let mut sandbox = sandbox_config_for_boot_spec(spec, format!("{prefix}-tpl"));
    sandbox.pmem_layers = vec![
        pmem_shared_support::per_vm_layer(shared_digest, 0),
        pmem_shared_support::per_vm_layer(per_vm_digest, 1),
    ];
    let hooks = snapshot_hooks(spec);
    let ready_probe = snapshot_ready_probe(spec);
    let inputs = backend
        .snapshot_template_inputs(&sandbox, hooks.clone())
        .expect("compute PerVm baseline template inputs");
    let fingerprint = TemplateFingerprint::compute(&inputs);
    let template_store = Arc::new(
        TemplateStore::create(template_store_root.to_path_buf(), admission_capacity)
            .expect("create PerVm baseline template store"),
    );
    let pool = WarmPool::new(
        backend,
        WarmPoolConfig {
            target_ready,
            sandbox,
            strategy: WarmStrategy::snapshot_restore(template_store, hooks, ready_probe),
            vm_id_prefix: prefix.to_owned(),
            cpu_allocator: Some(no_refill_allocator(target_ready)),
        },
    )
    .expect("WarmPool::new PerVm baseline");
    let before_fill_bytes = mem_available_before_checkpoint_bytes();
    pool.fill_to_target_blocking()
        .expect("fill PerVm baseline pool");
    let after_fill_bytes = mem_available_after_checkpoint_bytes();
    let fill_snapshot = pool.snapshot();
    assert_eq!(fill_snapshot.ready, target_ready);
    assert_eq!(fill_snapshot.fill_failures_total, 0);

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
            let mut lease = pool.try_lease().expect("lease PerVm baseline slot");
            start.wait();
            exercise_composed_lease(&mut lease, worker);
            tx.send(()).expect("send PerVm baseline result");
            hold.wait();
            lease.discard().expect("discard PerVm baseline lease");
        }));
    }
    drop(tx);
    start.wait();
    for _ in 0..target_ready {
        rx.recv().expect("receive PerVm baseline result");
    }
    let attached_snapshot = pool.snapshot();
    assert_attached_without_refill(attached_snapshot, target_ready);
    let after_n_attached_bytes = mem_available_after_checkpoint_bytes();
    hold.wait();
    for handle in handles {
        handle.join().expect("PerVm baseline lease thread");
    }
    let after_teardown_bytes = mem_available_before_checkpoint_bytes();
    drop(pool);
    snapshot_template_support::assert_no_run_dirs_with_prefix(run_root, prefix);

    let after_n_attached_delta_bytes = checked_mem_available_delta(
        before_fill_bytes,
        after_n_attached_bytes,
        "PerVm baseline attached checkpoint",
    );
    assert!(
        after_n_attached_delta_bytes > 0,
        "PerVm baseline must consume observable host memory"
    );
    let per_vm_payload_bytes = shared_image_bytes
        .checked_mul(target_ready as u64)
        .expect("shared image bytes * target_ready overflow");
    let per_vm_observed_bytes = after_n_attached_delta_bytes.div_ceil(target_ready as u64);
    let image_path = pmem_shared_support::store_erofs_path(image_store, per_vm_digest);
    let image_bytes = std::fs::metadata(&image_path)
        .expect("PerVm baseline image metadata")
        .len();
    PerVmBaseline {
        n_attached: target_ready,
        before_fill_bytes,
        after_fill_bytes,
        after_n_attached_bytes,
        after_teardown_bytes,
        after_n_attached_delta_bytes,
        per_vm_payload_bytes,
        per_vm_overhead_bytes: per_vm_observed_bytes,
        image_digest: per_vm_digest.as_str().to_owned(),
        image_bytes,
        shared_payload_digest: shared_digest.as_str().to_owned(),
        shared_payload_bytes: shared_image_bytes,
        template_fingerprint: fingerprint.to_hex(),
        attached_snapshot,
    }
}

pub(super) fn assert_attached_without_refill(snapshot: WarmPoolSnapshot, expected: usize) {
    assert_eq!(snapshot.leased, expected);
    assert_eq!(snapshot.ready, 0);
    assert_eq!(snapshot.filling, 0);
}

pub(super) fn checked_mem_available_delta(before: u64, after: u64, label: &str) -> u64 {
    before
        .checked_sub(after)
        .unwrap_or_else(|| panic!("{label}: MemAvailable increased from {before} to {after}"))
}
