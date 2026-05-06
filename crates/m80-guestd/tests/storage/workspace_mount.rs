use std::fs;
use std::path::Path;

#[test]
fn workspace_drive_is_vdc_after_overlay_pivot() {
    let source = pid_one_source();

    assert!(
        source.contains("const WORKSPACE_DEV: &str = \"/dev/vdc\";"),
        "PID-1 workspace mount must use the third drive, /dev/vdc"
    );
    assert!(
        !source.contains("const WORKSPACE_DEV: &str = \"/dev/vdb\";"),
        "PID-1 workspace mount must not reuse the overlay drive, /dev/vdb"
    );
}

#[test]
fn workspace_mount_happens_after_overlay_pivot() {
    let source = pid_one_source();
    let pivot_call = source
        .find("mount_overlay_and_pivot(boot_timer)")
        .expect("enter_pid_one_mode must call mount_overlay_and_pivot");
    let workspace_call = source
        .find("mount_workspace_if_present(boot_timer)")
        .expect("enter_pid_one_mode must call mount_workspace_if_present");

    assert!(
        pivot_call < workspace_call,
        "workspace mount must run after overlay pivot so /workspace is in the merged root"
    );
}

#[test]
fn missing_workspace_device_is_documented_optional() {
    let source = pid_one_source();

    assert!(
        source.contains("workspace_absent"),
        "PID-1 setup must keep the no-workspace skip milestone"
    );
    assert!(
        source.contains("skipping workspace mount"),
        "PID-1 setup must log the optional no-drive skip path"
    );
}

fn pid_one_source() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/pid_one.rs");
    fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}
