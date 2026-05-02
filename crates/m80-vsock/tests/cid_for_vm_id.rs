use m80_vsock::cid_for_vm_id;

#[test]
fn same_vm_id_yields_same_cid() {
    assert_eq!(cid_for_vm_id("vm-alpha"), cid_for_vm_id("vm-alpha"));
}

#[test]
fn different_vm_ids_yield_different_cids() {
    assert_ne!(cid_for_vm_id("vm-alpha"), cid_for_vm_id("vm-beta"));
    assert_ne!(cid_for_vm_id("vm-beta"), cid_for_vm_id("vm-gamma"));
    assert_ne!(cid_for_vm_id("vm-alpha"), cid_for_vm_id("vm-gamma"));
}

#[test]
fn cid_never_below_reserved_range() {
    for id in &["vm-alpha", "vm-beta", "vm-gamma", "a", "z", "", "0000"] {
        let cid = cid_for_vm_id(id);
        assert!(
            cid >= 3,
            "cid_for_vm_id({id:?}) = {cid} is below the minimum safe value of 3"
        );
    }
}

/// Pin specific CIDs so future hash-function refactors break this test
/// and force a deliberate review.
#[test]
fn pinned_cid_values() {
    // These values were computed from the SHA-256 implementation.
    // Changing the hash function must update these assertions.
    assert_eq!(cid_for_vm_id("vm-alpha"), 1_242_764_505);
    assert_eq!(cid_for_vm_id("vm-beta"), 2_563_786_887);
    assert_eq!(cid_for_vm_id("vm-gamma"), 2_483_998_716);
    assert_eq!(cid_for_vm_id("prod-01"), 2_903_356_710);
}
