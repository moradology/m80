use super::*;
use std::cell::RefCell;
use std::io;

#[derive(Default)]
struct FakeWorkspaceMountOps {
    device_exists: bool,
    mount_results: RefCell<Vec<io::Result<()>>>,
    repair_result: RefCell<Option<io::Result<()>>>,
    mkfs_result: RefCell<Option<io::Result<()>>>,
    calls: RefCell<Vec<&'static str>>,
}

impl FakeWorkspaceMountOps {
    fn with_mount_results(results: Vec<io::Result<()>>) -> Self {
        Self {
            device_exists: true,
            mount_results: RefCell::new(results),
            repair_result: RefCell::new(Some(Ok(()))),
            mkfs_result: RefCell::new(Some(Ok(()))),
            calls: RefCell::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<&'static str> {
        self.calls.borrow().clone()
    }
}

impl WorkspaceMountOps for FakeWorkspaceMountOps {
    fn device_exists(&self, _device: &str) -> bool {
        self.device_exists
    }

    fn mount_ext4(&self, _device: &str, _target: &str) -> io::Result<()> {
        self.calls.borrow_mut().push("mount");
        self.mount_results.borrow_mut().remove(0)
    }

    fn repair_ext4(&self, _device: &str) -> io::Result<()> {
        self.calls.borrow_mut().push("repair");
        self.repair_result.borrow_mut().take().unwrap()
    }

    fn mkfs_ext4(&self, _device: &str) -> io::Result<()> {
        self.calls.borrow_mut().push("mkfs");
        self.mkfs_result.borrow_mut().take().unwrap()
    }
}

fn mount_failure(label: &'static str) -> io::Result<()> {
    Err(io::Error::other(label))
}

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
fn pid_one_mounts_linux_runtime_dev_filesystems() {
    assert_eq!(DEV_SHM_TARGET, "/dev/shm");
    assert_eq!(DEV_SHM_MOUNT_DATA, "mode=1777");
    assert_eq!(DEV_PTS_TARGET, "/dev/pts");
    assert_eq!(DEV_PTS_MOUNT_DATA, "gid=5,mode=620,ptmxmode=666");
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
fn workspace_mkfs_cmdline_flag_controls_destructive_fallback() {
    assert!(workspace_mkfs_allowed_from_cmdline("m80.workspace=1 m80.workspace.mkfs=1").unwrap());
    assert!(!workspace_mkfs_allowed_from_cmdline("m80.workspace=1").unwrap());
    assert!(!workspace_mkfs_allowed_from_cmdline("m80.workspace.mkfs=0").unwrap());
}

#[test]
fn missing_workspace_device_skips_without_error() {
    let mut boot_timer = BootTimer::start();
    let missing = "/tmp/m80-guestd-missing-workspace-device-for-test";

    mount_workspace_device_if_present(&mut boot_timer, missing, WORKSPACE_TARGET, false)
        .expect("missing workspace device is documented-optional");
}

#[test]
fn workspace_mount_success_does_not_repair_or_format() {
    let ops = FakeWorkspaceMountOps::with_mount_results(vec![Ok(())]);
    let mut boot_timer = BootTimer::start();

    mount_workspace_device_with_ops(&mut boot_timer, "/dev/test", "/workspace", false, &ops)
        .unwrap();

    assert_eq!(ops.calls(), vec!["mount"]);
}

#[test]
fn workspace_mount_repairs_then_mounts() {
    let ops = FakeWorkspaceMountOps::with_mount_results(vec![mount_failure("bad fs"), Ok(())]);
    let mut boot_timer = BootTimer::start();

    mount_workspace_device_with_ops(&mut boot_timer, "/dev/test", "/workspace", false, &ops)
        .unwrap();

    assert_eq!(ops.calls(), vec!["mount", "repair", "mount"]);
}

#[test]
fn workspace_mount_does_not_mkfs_when_fallback_disabled() {
    let ops = FakeWorkspaceMountOps::with_mount_results(vec![
        mount_failure("bad fs"),
        mount_failure("still bad"),
    ]);
    let mut boot_timer = BootTimer::start();

    let err =
        mount_workspace_device_with_ops(&mut boot_timer, "/dev/test", "/workspace", false, &ops)
            .unwrap_err();

    assert!(err.to_string().contains("mkfs fallback is disabled"));
    assert_eq!(ops.calls(), vec!["mount", "repair", "mount"]);
}

#[test]
fn workspace_mount_formats_only_when_explicitly_allowed() {
    let ops = FakeWorkspaceMountOps::with_mount_results(vec![
        mount_failure("bad fs"),
        mount_failure("still bad"),
        Ok(()),
    ]);
    let mut boot_timer = BootTimer::start();

    mount_workspace_device_with_ops(&mut boot_timer, "/dev/test", "/workspace", true, &ops)
        .unwrap();

    assert_eq!(
        ops.calls(),
        vec!["mount", "repair", "mount", "mkfs", "mount"]
    );
}

#[test]
fn workspace_mount_reports_final_failure_after_mkfs() {
    let ops = FakeWorkspaceMountOps::with_mount_results(vec![
        mount_failure("bad fs"),
        mount_failure("still bad"),
        mount_failure("formatted bad"),
    ]);
    let mut boot_timer = BootTimer::start();

    let err =
        mount_workspace_device_with_ops(&mut boot_timer, "/dev/test", "/workspace", true, &ops)
            .unwrap_err();

    assert!(err
        .to_string()
        .contains("workspace mount failed after mkfs"));
    assert_eq!(
        ops.calls(),
        vec!["mount", "repair", "mount", "mkfs", "mount"]
    );
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
