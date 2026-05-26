//! Scenario 1: Shared pmem VMs bind the same canonical store inode.

mod common;
mod pmem_shared_support;

use pmem_shared_support as support;

#[test]
#[ignore = "requires-kvm requires-root requires-artifacts requires-erofs-tool"]
fn shared_pmem_inode_identity_matches_store_while_per_vm_uses_clones() {
    let _serial = support::REAL_KVM_LOCK.lock().expect("real-kvm test lock");
    let store = support::open_default_store();
    let digest = support::build_test_image_digest(&store, 0);
    let store_path = support::store_erofs_path(&store, &digest);
    let store_identity = support::file_identity(&store_path);
    let real = support::real_backend(2);

    let mut first_shared = support::launch_with_layers(
        &real.backend,
        "pmem-shared-id-a",
        vec![support::shared_layer(&digest, 0)],
    );
    let mut second_shared = support::launch_with_layers(
        &real.backend,
        "pmem-shared-id-b",
        vec![support::shared_layer(&digest, 0)],
    );
    let first_shared_run_dir = first_shared.run_dir().to_path_buf();
    let second_shared_run_dir = second_shared.run_dir().to_path_buf();

    let first_mount = support::assert_one_pmem_mount(&mut first_shared, 0);
    let second_mount = support::assert_one_pmem_mount(&mut second_shared, 0);
    assert_eq!(
        support::file_identity(&support::jail_backing_path(
            &first_shared_run_dir,
            &real.firecracker_bin,
            0,
        )),
        store_identity
    );
    assert_eq!(
        support::file_identity(&support::jail_backing_path(
            &second_shared_run_dir,
            &real.firecracker_bin,
            0,
        )),
        store_identity
    );

    support::stop_and_delete(first_shared);
    support::stop_and_delete(second_shared);

    let first_per_vm = support::launch_with_layers(
        &real.backend,
        "pmem-pervm-id-a",
        vec![support::per_vm_layer(&digest, 0)],
    );
    let second_per_vm = support::launch_with_layers(
        &real.backend,
        "pmem-pervm-id-b",
        vec![support::per_vm_layer(&digest, 0)],
    );
    let first_per_vm_backing = first_per_vm.run_dir().join("pmem/0.img");
    let second_per_vm_backing = second_per_vm.run_dir().join("pmem/0.img");
    assert_ne!(
        support::file_identity(&first_per_vm_backing),
        support::file_identity(&second_per_vm_backing),
        "PerVm controls must use distinct backing inodes"
    );
    assert_ne!(
        support::file_identity(&first_per_vm_backing),
        store_identity,
        "PerVm control must not bind the canonical Shared artifact"
    );
    assert_ne!(
        support::file_identity(&second_per_vm_backing),
        store_identity,
        "PerVm control must not bind the canonical Shared artifact"
    );

    support::stop_and_delete(first_per_vm);
    support::stop_and_delete(second_per_vm);
    println!(
        "shared_pmem_inode_identity_matches_store_while_per_vm_uses_clones passed: store_path={} store_dev={} store_inode={} first_mount_line={} second_mount_line={}",
        store_path.display(),
        store_identity.0,
        store_identity.1,
        first_mount,
        second_mount
    );
}
