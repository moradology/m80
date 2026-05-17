//! Scenario 4: Shared and PerVm layers can reference one digest without crosstalk.

mod common;
mod pmem_shared_support;

use pmem_shared_support as support;

#[test]
#[ignore = "requires KVM host, real Firecracker binary, and root privileges"]
fn mixed_shared_and_per_vm_same_digest_keep_distinct_backing_policies() {
    let _serial = support::REAL_KVM_LOCK.lock().expect("real-kvm test lock");
    let store = support::open_default_store();
    let digest = support::build_test_image_digest(&store, 0);
    let store_path = support::store_erofs_path(&store, &digest);
    let store_identity = support::file_identity(&store_path);
    let real = support::real_backend(2);

    let mut shared = support::launch_with_layers(
        &real.backend,
        "pmem-mixed-shared",
        vec![support::shared_layer(&digest, 0)],
    );
    let mut per_vm = support::launch_with_layers(
        &real.backend,
        "pmem-mixed-pervm",
        vec![support::per_vm_layer(&digest, 0)],
    );
    let shared_run_dir = shared.run_dir().to_path_buf();
    let per_vm_run_dir = per_vm.run_dir().to_path_buf();
    let per_vm_backing = per_vm_run_dir.join("pmem/0.img");

    let shared_mount = support::assert_one_pmem_mount(&mut shared, 0);
    let per_vm_mount = support::assert_one_pmem_mount(&mut per_vm, 0);
    assert_eq!(
        support::file_identity(&support::jail_backing_path(
            &shared_run_dir,
            &real.firecracker_bin,
            0,
        )),
        store_identity,
        "Shared VM must bind the canonical store artifact"
    );
    assert!(
        !shared_run_dir.join("pmem/0.img").exists(),
        "Shared VM must not create a per-VM backing clone"
    );
    assert!(
        per_vm_backing.is_file(),
        "PerVm VM must create its own backing clone"
    );
    assert_ne!(
        support::file_identity(&per_vm_backing),
        store_identity,
        "PerVm VM must not bind the canonical store artifact"
    );
    assert_eq!(
        store.shared_ref_count(&digest).unwrap(),
        1,
        "only the Shared VM should hold an active-use marker"
    );

    let shared_payload =
        support::exec_stdout(&mut shared, "cat /opt/m80-layers/smoke-0/payload.txt");
    let per_vm_payload =
        support::exec_stdout(&mut per_vm, "cat /opt/m80-layers/smoke-0/payload.txt");
    assert_eq!(shared_payload, per_vm_payload);

    support::stop_and_delete(shared);
    assert_eq!(
        store.shared_ref_count(&digest).unwrap(),
        0,
        "stopping the Shared VM must release its marker"
    );
    assert!(store_path.is_file(), "canonical artifact must remain");
    support::stop_and_delete(per_vm);
    assert!(!shared_run_dir.exists(), "Shared run dir leaked");
    assert!(!per_vm_run_dir.exists(), "PerVm run dir leaked");
    println!(
        "mixed_shared_and_per_vm_same_digest_keep_distinct_backing_policies passed: store_path={} shared_mount_line={} per_vm_mount_line={}",
        store_path.display(),
        shared_mount,
        per_vm_mount
    );
}
