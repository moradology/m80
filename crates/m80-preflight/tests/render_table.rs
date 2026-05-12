//! Verify the Discovery::render_table output contains expected labels and
//! status markers for each fixture row.

use m80_image_manifest::{ImageKind, KernelKind, Manifest};
use m80_preflight::{CheckRow, Discovery, PrivilegeStatus};
use std::path::PathBuf;

fn fixture_manifest() -> Manifest {
    Manifest::new(
        PathBuf::from("/usr/local/bin/guestd"),
        "a".repeat(64),
        "v1.15.1".into(),
        3000,
        ImageKind::Ubuntu,
        PathBuf::from("/opt/m80/artifacts/vmlinux-6.1"),
        "b".repeat(64),
        KernelKind::Stock,
        None,
        PathBuf::from("/opt/m80/images/rootfs.ext4"),
        "c".repeat(64),
        "READY".into(),
        Some(PathBuf::from("/opt/m80/images/source.ext4")),
        Some("e".repeat(64)),
    )
}

fn fixture_discovery() -> Discovery {
    let rows = vec![
        CheckRow {
            label: "OS gate".into(),
            passed: true,
            detail: "Linux 6.1.0".into(),
        },
        CheckRow {
            label: "KVM".into(),
            passed: true,
            detail: "/dev/kvm present".into(),
        },
        CheckRow {
            label: "Kernel modules".into(),
            passed: false,
            detail: "tap missing".into(),
        },
    ];
    Discovery {
        firecracker_bin: PathBuf::from("/opt/firecracker/bin/firecracker"),
        jailer_bin: PathBuf::from("/opt/firecracker/bin/jailer"),
        jailer_harden_bin: PathBuf::from("/opt/m80/bin/m80-jailer-harden"),
        kernel: PathBuf::from("/opt/m80/artifacts/vmlinux-6.1"),
        rootfs: PathBuf::from("/opt/m80/images/rootfs.ext4"),
        manifest: fixture_manifest(),
        run_root: PathBuf::from("/var/run/m80"),
        privilege: PrivilegeStatus::Root,
        report: rows,
    }
}

#[test]
fn render_contains_pass_and_fail_markers() {
    let table = fixture_discovery().render_table();
    assert!(table.contains("PASS"), "missing PASS:\n{table}");
    assert!(table.contains("FAIL"), "missing FAIL:\n{table}");
}

#[test]
fn render_contains_all_labels() {
    let d = fixture_discovery();
    let table = d.render_table();
    for row in &d.report {
        assert!(
            table.contains(row.label.as_str()),
            "label '{}' must appear in rendered table; got:\n{table}",
            row.label
        );
    }
}

#[test]
fn render_contains_detail_text() {
    let d = fixture_discovery();
    let table = d.render_table();
    assert!(
        table.contains("Linux 6.1.0"),
        "detail text must appear in rendered table; got:\n{table}"
    );
}

#[test]
fn render_is_newline_terminated() {
    let d = fixture_discovery();
    let table = d.render_table();
    assert!(
        table.ends_with('\n'),
        "rendered table must be newline-terminated"
    );
}
