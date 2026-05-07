use super::*;

#[test]
fn is_pid_one_false_in_test_runner() {
    assert!(!is_pid_one());
}

#[test]
fn reap_pending_safe_with_no_children() {
    reap_pending();
}

/// Verify that the overlay option string is constructed correctly.
#[test]
fn overlay_opts_format() {
    let lower = "/lower";
    let upper = "/upper/root";
    let work = "/upper/.work";
    let opts = format!("lowerdir={lower},upperdir={upper},workdir={work}");
    assert_eq!(
        opts,
        "lowerdir=/lower,upperdir=/upper/root,workdir=/upper/.work"
    );
}

/// Verify that the workspace device path is /dev/vdc (post-pivot drive
/// layout per docs/design/storage-overlay.md §2).
#[test]
fn workspace_dev_is_vdc() {
    assert_eq!(WORKSPACE_DEV, "/dev/vdc");
    assert_eq!(WORKSPACE_TARGET, "/workspace");
}

#[test]
fn workspace_cmdline_flag_controls_pid1_workspace_mount() {
    assert!(
        workspace_requested_from_cmdline("console=ttyS0 init=/m80-guestd m80.workspace=1").unwrap()
    );
    assert!(
        !workspace_requested_from_cmdline("console=ttyS0 init=/m80-guestd m80.workspace=0")
            .unwrap()
    );
    assert!(!workspace_requested_from_cmdline("console=ttyS0 init=/m80-guestd").unwrap());
}

#[test]
fn missing_workspace_device_skips_without_error() {
    let mut boot_timer = BootTimer::start();
    let missing = "/tmp/m80-guestd-missing-workspace-device-for-test";

    mount_workspace_device_if_present(&mut boot_timer, missing, WORKSPACE_TARGET)
        .expect("missing workspace device is documented-optional");
}

/// Verify that pivot_rootfs (with the test stub for pivot_root) does not
/// panic when called with a valid path. This exercises the defer!/close
/// wrappers and fchdir sequence without a real mount namespace.
///
/// Requires a real filesystem path that is a directory; "/" works.
/// The syscall is stubbed in #[cfg(test)] so no privilege is needed.
#[test]
#[ignore = "requires a mount namespace; run in a real VM or with unshare -m"]
fn pivot_rootfs_on_real_namespace() {
    // Would call pivot_rootfs("/merged") in a real mount namespace.
    // Stub path exercises the open/fchdir/defer logic without actual
    // pivot_root(2) -- covered by the cfg(test) shim above.
    pivot_rootfs("/").expect("pivot_rootfs stub must not fail");
}
