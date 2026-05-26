const CGROUP: &str = include_str!("../../src/lib.rs");

#[test]
fn subtree_drop_warnings_include_vm_id() {
    assert_source_contains(
        CGROUP,
        &[
            "pub struct Subtree",
            "vm_id: String",
            "vm_id: vm_id.to_owned(),",
            "vm_id = %self.vm_id",
            "drop: cgroup.kill failed",
            "drop: cgroup rmdir failed",
        ],
    );
}

fn assert_source_contains(source: &str, needles: &[&str]) {
    for needle in needles {
        assert!(source.contains(needle), "missing source needle: {needle}");
    }
}
