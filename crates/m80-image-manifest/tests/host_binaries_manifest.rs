use std::path::PathBuf;

use m80_image_manifest::{
    HostBinariesManifest, HostBinaryEntry, HostBinaryName, HostLaunchMaterialEntry,
    HostLaunchMaterialName, ManifestError, HOST_BINARIES_SCHEMA_VERSION,
};

fn fixture_manifest() -> HostBinariesManifest {
    HostBinariesManifest::new(
        vec![
            HostBinaryEntry {
                name: HostBinaryName::Firecracker,
                path: PathBuf::from("/opt/firecracker/bin/firecracker"),
                sha256: "a".repeat(64),
            },
            HostBinaryEntry {
                name: HostBinaryName::Jailer,
                path: PathBuf::from("/opt/firecracker/bin/jailer"),
                sha256: "b".repeat(64),
            },
            HostBinaryEntry {
                name: HostBinaryName::M80,
                path: PathBuf::from("/opt/m80/bin/m80"),
                sha256: "c".repeat(64),
            },
            HostBinaryEntry {
                name: HostBinaryName::M80Cli,
                path: PathBuf::from("/opt/m80/bin/m80-cli"),
                sha256: "d".repeat(64),
            },
            HostBinaryEntry {
                name: HostBinaryName::M80JailerHarden,
                path: PathBuf::from("/opt/m80/bin/m80-jailer-harden"),
                sha256: "e".repeat(64),
            },
            HostBinaryEntry {
                name: HostBinaryName::M80NetHelper,
                path: PathBuf::from("/opt/m80/bin/m80-net-helper"),
                sha256: "f".repeat(64),
            },
        ],
        vec![HostLaunchMaterialEntry {
            name: HostLaunchMaterialName::FirecrackerSeccompFilter,
            path: PathBuf::from("/opt/firecracker/bin/firecracker-seccomp-filter.bin"),
            sha256: "0".repeat(64),
        }],
    )
}

#[test]
fn host_binaries_manifest_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("host-binaries.manifest.json");
    let manifest = fixture_manifest();

    manifest.write(&path).unwrap();
    let read = HostBinariesManifest::read(&path).unwrap();

    assert_eq!(read, manifest);
    assert_eq!(read.schema_version(), HOST_BINARIES_SCHEMA_VERSION);
}

#[test]
fn host_binaries_manifest_rejects_unknown_schema() {
    let raw = br#"{
  "binaries": [],
  "launch_material": [],
  "schema_version": 99
}"#;

    let err = HostBinariesManifest::from_bytes(raw).unwrap_err();

    assert!(matches!(
        err,
        ManifestError::UnsupportedHostBinariesSchemaVersion(99)
    ));
}

#[test]
fn host_binaries_manifest_rejects_v2_without_launch_material() {
    let raw = br#"{
  "binaries": [],
  "schema_version": 2
}"#;

    let err = HostBinariesManifest::from_bytes(raw).unwrap_err();

    assert!(matches!(
        err,
        ManifestError::UnsupportedHostBinariesSchemaVersion(2)
    ));
}

#[test]
fn host_binaries_manifest_rejects_unknown_fields() {
    let raw = br#"{
  "binaries": [],
  "future_field": true,
  "launch_material": [],
  "schema_version": 3
}"#;

    let err = HostBinariesManifest::from_bytes(raw).unwrap_err();

    assert!(matches!(err, ManifestError::Json(_)));
}
