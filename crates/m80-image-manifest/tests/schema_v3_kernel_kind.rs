//! Schema v4 retains KernelKind field — default (absent in JSON) and Stripped roundtrip.

mod common;

use m80_image_manifest::{KernelKind, Manifest, SCHEMA_VERSION};

/// When `kernel_kind` is absent from the JSON (as in an older document that
/// had its `schema_version` bumped to the current value for testing), it must deserialize as
/// `KernelKind::Stock` via `#[serde(default)]`.
#[test]
fn kernel_kind_absent_defaults_to_stock() {
    let dir = tempfile::tempdir().unwrap();

    // Build a JSON blob with the current schema_version but no kernel_kind field.
    // We write the artifact files so verify could run, but we only care about
    // the deserialized value here.
    for name in &["vmlinux", "source.ext4", "output.ext4", "guestd"] {
        std::fs::write(dir.path().join(name), name.as_bytes()).unwrap();
    }

    use sha2::{Digest, Sha256};
    let json = format!(
        r#"{{
  "daemon_binary_path": "{dir}/guestd",
  "daemon_binary_sha256": "{guestd_sha}",
  "expected_firecracker_version": "v1.15.1",
  "guest_port": 8080,
  "image_kind": "ubuntu",
  "kernel_image": "{dir}/vmlinux",
  "kernel_image_sha256": "{vmlinux_sha}",
  "no_egress_reason": null,
  "output_rootfs_image": "{dir}/output.ext4",
  "output_rootfs_sha256": "{output_sha}",
  "ready_marker": "READY",
  "schema_version": {schema},
  "source_rootfs_image": "{dir}/source.ext4",
  "source_rootfs_sha256": "{source_sha}"
}}"#,
        dir = dir.path().display(),
        guestd_sha = hex::encode(Sha256::digest(b"guestd")),
        vmlinux_sha = hex::encode(Sha256::digest(b"vmlinux")),
        output_sha = hex::encode(Sha256::digest(b"output.ext4")),
        source_sha = hex::encode(Sha256::digest(b"source.ext4")),
        schema = SCHEMA_VERSION,
    );

    let path = dir.path().join("m.json");
    std::fs::write(&path, json.as_bytes()).unwrap();

    let m = Manifest::read(&path).unwrap();
    assert_eq!(
        m.kernel_kind,
        KernelKind::Stock,
        "absent kernel_kind must default to Stock"
    );
}

/// `KernelKind::Stripped` roundtrips through write → read without loss.
#[test]
fn kernel_kind_stripped_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = common::make_artifacts(dir.path());
    m.kernel_kind = KernelKind::Stripped;

    let path = dir.path().join("stripped.json");
    m.write(&path).unwrap();

    let m2 = Manifest::read(&path).unwrap();
    assert_eq!(m2.kernel_kind, KernelKind::Stripped);
}

/// `KernelKind` default is `Stock` (Rust Default trait).
#[test]
fn kernel_kind_default_is_stock() {
    assert_eq!(KernelKind::default(), KernelKind::Stock);
}
