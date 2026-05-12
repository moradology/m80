use m80_cgroup::cleanup_orphan_subtree;

#[test]
#[ignore = "requires writable cgroup hierarchy"]
fn cleanup_idempotent() {
    let vm_id = format!("m80-test-missing-{}", std::process::id());

    cleanup_orphan_subtree(&vm_id).expect("missing subtree cleanup must be idempotent");
}
