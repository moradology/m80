//! Scenario 5: Shared active-use markers drop per teardown without artifact GC.

mod common;
mod pmem_shared_support;

use pmem_shared_support as support;

#[test]
#[ignore = "requires-kvm requires-root requires-artifacts requires-erofs-tool requires-pmem"]
fn shared_pmem_marker_lifecycle_releases_refs_and_preserves_artifact() {
    let _serial = support::REAL_KVM_LOCK.lock().expect("real-kvm test lock");
    let store = support::open_default_store();
    let digest = support::build_test_image_digest(&store, 0);
    let store_path = support::store_erofs_path(&store, &digest);
    let store_identity = support::file_identity(&store_path);
    let real = support::real_backend(3);

    let mut running = Vec::new();
    for idx in 0..3 {
        let mut sandbox = support::launch_with_layers(
            &real.backend,
            &format!("pmem-shared-ref-{idx}"),
            vec![support::shared_layer(&digest, 0)],
        );
        support::assert_one_pmem_mount(&mut sandbox, 0);
        assert_eq!(
            support::file_identity(&support::jail_backing_path(
                sandbox.run_dir(),
                &real.firecracker_bin,
                0,
            )),
            store_identity
        );
        running.push(sandbox);
    }
    assert_eq!(store.shared_ref_count(&digest).unwrap(), 3);

    while let Some(sandbox) = running.pop() {
        let expected_after_stop = running.len();
        support::stop_and_delete(sandbox);
        assert_eq!(
            store.shared_ref_count(&digest).unwrap(),
            expected_after_stop,
            "teardown must release exactly one Shared marker"
        );
        assert!(
            store_path.is_file(),
            "canonical Shared artifact must remain after marker release"
        );
    }

    let _stale = store
        .acquire_shared_ref(&digest, "pmem-shared-stale-marker")
        .expect("create stale marker");
    assert_eq!(store.shared_ref_count(&digest).unwrap(), 1);
    let removed = store
        .sweep_shared_refs(std::iter::empty::<&str>())
        .expect("sweep stale marker");
    assert_eq!(removed, 1);
    assert_eq!(store.shared_ref_count(&digest).unwrap(), 0);
    assert!(store_path.is_file(), "sweep must not delete artifact");
    println!(
        "shared_pmem_marker_lifecycle_releases_refs_and_preserves_artifact passed: store_path={} store_dev={} store_inode={}",
        store_path.display(),
        store_identity.0,
        store_identity.1
    );
}
