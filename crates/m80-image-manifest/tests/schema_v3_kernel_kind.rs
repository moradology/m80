//! Schema v4 KernelKind field roundtrip and default.

mod common;

use m80_image_manifest::KernelKind;

/// `KernelKind::Stripped` roundtrips through write → read without loss.
#[test]
fn kernel_kind_stripped_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = common::make_artifacts(dir.path());
    m.kernel_kind = KernelKind::Stripped;

    let path = dir.path().join("stripped.json");
    m.write(&path).unwrap();

    let m2 = m80_image_manifest::Manifest::read(&path).unwrap();
    assert_eq!(m2.kernel_kind, KernelKind::Stripped);
}

/// `KernelKind` default is `Stock` (Rust Default trait).
#[test]
fn kernel_kind_default_is_stock() {
    assert_eq!(KernelKind::default(), KernelKind::Stock);
}
