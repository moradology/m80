use std::path::PathBuf;

use m80_image_manifest::{
    BuildReceipt, BuildReceiptArtifact, BuildReceiptArtifactKind, ManifestError,
    BUILD_RECEIPT_SCHEMA_VERSION,
};

fn fixture_receipt() -> BuildReceipt {
    BuildReceipt::new(
        PathBuf::from("/opt/m80/artifacts/output.ext4.manifest.json"),
        "a".repeat(64),
        vec![
            BuildReceiptArtifact {
                kind: BuildReceiptArtifactKind::KernelImage,
                path: PathBuf::from("/opt/m80/artifacts/vmlinux"),
                sha256: "b".repeat(64),
            },
            BuildReceiptArtifact {
                kind: BuildReceiptArtifactKind::OutputRootfsImage,
                path: PathBuf::from("/opt/m80/artifacts/output.ext4"),
                sha256: "c".repeat(64),
            },
            BuildReceiptArtifact {
                kind: BuildReceiptArtifactKind::DaemonBinaryPath,
                path: PathBuf::from("/opt/m80/artifacts/m80-guestd"),
                sha256: "d".repeat(64),
            },
        ],
    )
}

#[test]
fn build_receipt_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("output.ext4.build-receipt.json");
    let receipt = fixture_receipt();

    receipt.write(&path).unwrap();
    let read = BuildReceipt::read(&path).unwrap();

    assert_eq!(read, receipt);
    assert_eq!(read.schema_version(), BUILD_RECEIPT_SCHEMA_VERSION);
}

#[test]
fn build_receipt_rejects_unknown_schema() {
    let raw = br#"{
  "artifacts": [],
  "manifest_path": "/x/output.ext4.manifest.json",
  "manifest_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "schema_version": 99
}"#;

    let err = BuildReceipt::from_bytes(raw).unwrap_err();

    assert!(matches!(
        err,
        ManifestError::UnsupportedBuildReceiptSchemaVersion(99)
    ));
}

#[test]
fn build_receipt_rejects_unknown_fields() {
    let raw = br#"{
  "artifacts": [],
  "future_field": true,
  "manifest_path": "/x/output.ext4.manifest.json",
  "manifest_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "schema_version": 1
}"#;

    let err = BuildReceipt::from_bytes(raw).unwrap_err();

    assert!(matches!(err, ManifestError::Json(_)));
}
