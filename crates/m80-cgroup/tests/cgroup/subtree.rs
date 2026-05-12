use std::path::PathBuf;

use m80_cgroup::Subtree;

#[test]
fn leaf_under_renamed_root() {
    assert_eq!(
        Subtree::leaf_path("vm-1"),
        PathBuf::from("/sys/fs/cgroup/m80-firecracker/vm-1")
    );
}
