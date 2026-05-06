use m80_cgroup::{cleanup_orphan_subtree, Subtree};

#[test]
fn requires_jailer() {
    let _create: fn(
        &str,
        &m80_jailer::MaterializedJail,
        &m80_jailer::JailedFirecracker,
    ) -> Result<Subtree, m80_cgroup::CgroupError> = Subtree::create;

    // If Subtree::create ever stops requiring jailed launch evidence, the
    // function-pointer assignment above stops compiling.
}

#[test]
fn cleanup_idempotent() {
    let vm_id = format!("m80-test-missing-{}", std::process::id());

    cleanup_orphan_subtree(&vm_id).expect("missing subtree cleanup must be idempotent");
}
