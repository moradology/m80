//! Bead m80-sz1.3.4 — boot_target, guest_port, ready_marker in manifest
//!
//! Doc anchor: `docs/behaviors/image-build/manifest.md#boot-fields`

mod common;

use m80_image_manifest::Manifest;

/// Bead m80-sz1.3.4: boot_target, guest_port, and ready_marker survive the
/// write → read round-trip so the host can derive vsock and serial-probe
/// parameters without re-running the build.
#[test]
fn records_boot_target_port_marker() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = common::make_artifacts(dir.path());
    m.guest_port = 52000;
    m.ready_marker = "AGENT_DAEMON_READY".into();

    let path = dir.path().join("rootfs.ext4.manifest.json");
    m.write(&path).unwrap();
    let m2 = Manifest::read(&path).unwrap();

    assert_eq!(m2.boot_target.as_deref(), Some("multi-user.target"));
    assert_eq!(m2.guest_port, 52000);
    assert_eq!(m2.ready_marker, "AGENT_DAEMON_READY");
}
