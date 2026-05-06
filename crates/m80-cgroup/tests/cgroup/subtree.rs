use std::path::PathBuf;

use m80_cgroup::Subtree;

#[test]
fn leaf_under_renamed_root() {
    assert_eq!(
        Subtree::leaf_path("vm-1"),
        PathBuf::from("/sys/fs/cgroup/m80-firecracker/vm-1")
    );
}

#[test]
fn pids_assigned_to_leaf() {
    let leaf = Subtree::leaf_path("vm-pids");
    assert_eq!(
        leaf.join("cgroup.procs"),
        PathBuf::from("/sys/fs/cgroup/m80-firecracker/vm-pids/cgroup.procs")
    );
}
