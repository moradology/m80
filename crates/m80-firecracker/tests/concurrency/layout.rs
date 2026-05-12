use std::path::Path;

use m80_firecracker::{
    boot_identity_path, console_log_path, rootfs_overlay_path, run_dir_path, scratch_image_path,
    OWNERSHIP_LOCK,
};

#[test]
fn state_lives_under_run_root_slash_vm_id() {
    let run_root = Path::new("/var/run/m80");
    let run_dir = run_dir_path(run_root, "vm-a");

    assert_eq!(run_dir, run_root.join("vm-a"));
    assert_eq!(
        rootfs_overlay_path(&run_dir),
        run_dir.join("rootfs.overlay.ext4")
    );
    assert_eq!(scratch_image_path(&run_dir), run_dir.join("scratch.ext4"));
    assert_eq!(console_log_path(&run_dir), run_dir.join("console.log"));
    assert_eq!(
        boot_identity_path(&run_dir),
        run_dir.join("boot-identity.json")
    );
    assert_eq!(
        run_dir.join(OWNERSHIP_LOCK),
        run_root.join("vm-a").join("ownership.lock")
    );
}
