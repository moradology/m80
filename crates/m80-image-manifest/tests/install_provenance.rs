use std::path::PathBuf;

use m80_image_manifest::{
    InstallProvenance, InstallProvenanceArtifact, InstallProvenanceRewrite,
    InstallProvenanceTransform, ManifestError, INSTALL_PROVENANCE_SCHEMA_VERSION,
};

fn fixture_provenance() -> InstallProvenance {
    InstallProvenance::new(
        Some("v0.1.2".to_owned()),
        vec![
            InstallProvenanceTransform {
                artifact: InstallProvenanceArtifact::GuestManifest,
                source_sha256: "a".repeat(64),
                source_path: PathBuf::from("artifacts/output.ext4.manifest.json"),
                installed_sha256: "b".repeat(64),
                installed_path: PathBuf::from("/opt/m80/artifacts/output.ext4.manifest.json"),
                rewrite: InstallProvenanceRewrite::InstallPathRewrite,
            },
            InstallProvenanceTransform {
                artifact: InstallProvenanceArtifact::BuildReceipt,
                source_sha256: "c".repeat(64),
                source_path: PathBuf::from("artifacts/output.ext4.build-receipt.json"),
                installed_sha256: "d".repeat(64),
                installed_path: PathBuf::from("/opt/m80/artifacts/output.ext4.build-receipt.json"),
                rewrite: InstallProvenanceRewrite::InstallPathRewrite,
            },
        ],
    )
}

#[test]
fn install_provenance_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("install-provenance.json");
    let provenance = fixture_provenance();

    provenance.write(&path).unwrap();
    let read = InstallProvenance::read(&path).unwrap();

    assert_eq!(read, provenance);
    assert_eq!(read.schema_version(), INSTALL_PROVENANCE_SCHEMA_VERSION);
}

#[test]
fn install_provenance_rejects_unknown_schema() {
    let raw = br#"{
  "release_tag": "v0.1.2",
  "transforms": [],
  "schema_version": 99
}"#;

    let err = InstallProvenance::from_bytes(raw).unwrap_err();

    assert!(matches!(
        err,
        ManifestError::UnsupportedInstallProvenanceSchemaVersion(99)
    ));
}

#[test]
fn install_provenance_rejects_unknown_fields() {
    let raw = br#"{
  "future_field": true,
  "release_tag": "v0.1.2",
  "transforms": [],
  "schema_version": 1
}"#;

    let err = InstallProvenance::from_bytes(raw).unwrap_err();

    assert!(matches!(err, ManifestError::Json(_)));
}
