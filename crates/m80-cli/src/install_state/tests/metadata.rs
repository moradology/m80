use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use m80_image_manifest::{
    HostBinariesManifest, HostBinaryEntry, HostBinaryName, HostLaunchMaterialEntry,
    HostLaunchMaterialName, InstallProvenance, InstallProvenanceArtifact, InstallProvenanceRewrite,
    InstallProvenanceTransform,
};
use sha2::{Digest, Sha256};

use super::*;

mod proof_fixture;

const TAG: &str = "v1.2.3";

#[test]
fn resolver_reports_missing_install_metadata() {
    let fixture = InstallStateFixture::new();
    fixture.write_installed_profile(TAG);
    fixture.write_system_config("default");
    fixture.point_active_at(TAG);

    let report = fixture.resolve(None);

    assert_eq!(report.state, InstallStateKind::MissingInstallMetadata);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == InstallStateDiagnosticCode::InstallMetadataMissing
            && diagnostic.field == Some("bundle_metadata")
    }));
}

#[test]
fn resolver_rejects_unknown_bundle_metadata_field() {
    let fixture = installed_fixture();
    mutate_json(&bundle_path(&fixture), |value| {
        value["surprise"] = serde_json::json!(true);
    });

    let report = fixture.resolve(None);

    assert_eq!(report.state, InstallStateKind::InvalidInstallMetadata);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == InstallStateDiagnosticCode::InstallMetadataInvalid
            && diagnostic.field == Some("bundle_metadata")
    }));
}

#[test]
fn resolver_rejects_bad_bundle_digest_syntax() {
    let fixture = installed_fixture();
    mutate_json(&bundle_path(&fixture), |value| {
        value["files"][0]["sha256"] = serde_json::json!("not-a-digest");
    });

    let report = fixture.resolve(None);

    assert_eq!(report.state, InstallStateKind::InvalidInstallMetadata);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == InstallStateDiagnosticCode::InstallMetadataInvalid
            && diagnostic.message.contains("64-hex sha256")
    }));
}

#[test]
fn resolver_rejects_absolute_bundle_file_path() {
    let fixture = installed_fixture();
    mutate_json(&bundle_path(&fixture), |value| {
        value["files"][0]["path"] = serde_json::json!("/tmp/escape");
    });

    let report = fixture.resolve(None);

    assert_eq!(report.state, InstallStateKind::InvalidInstallMetadata);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == InstallStateDiagnosticCode::InstallMetadataInvalid
            && diagnostic.message.contains("must be relative")
    }));
}

#[test]
fn resolver_reports_stale_metadata_for_missing_bundle_reference() {
    let fixture = installed_fixture();
    fs::remove_file(version_dir(&fixture).join("artifacts/output.ext4"))
        .expect("remove bundle reference");

    let report = fixture.resolve(None);

    assert_eq!(report.state, InstallStateKind::StaleInstallMetadata);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == InstallStateDiagnosticCode::InstallMetadataStale
            && diagnostic.field == Some("bundle_metadata")
    }));
}

#[test]
fn resolver_rejects_bundle_reference_symlink_even_when_digest_matches() {
    let fixture = installed_fixture();
    let installed = version_dir(&fixture).join("artifacts/output.ext4");
    let outside = fixture.temp.path().join("outside-output.ext4");
    fs::copy(&installed, &outside).expect("copy bundle reference outside version dir");
    fs::remove_file(&installed).expect("remove installed bundle reference");
    symlink(&outside, &installed).expect("replace bundle reference with symlink");

    let report = fixture.resolve(None);

    assert_eq!(report.state, InstallStateKind::StaleInstallMetadata);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == InstallStateDiagnosticCode::InstallMetadataStale
            && diagnostic.field == Some("bundle_metadata")
            && diagnostic.message.contains("must not traverse symlink")
    }));
}

#[test]
fn resolver_rejects_bad_install_provenance_digest_syntax() {
    let fixture = installed_fixture();
    mutate_json(&install_provenance_path(&fixture), |value| {
        value["transforms"][0]["installed_sha256"] = serde_json::json!("not-a-digest");
    });

    let report = fixture.resolve(None);

    assert_eq!(report.state, InstallStateKind::StaleInstallMetadata);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == InstallStateDiagnosticCode::InstallMetadataStale
            && diagnostic.field == Some("install_provenance")
            && diagnostic.message.contains("64-hex sha256")
    }));
}

#[test]
fn resolver_rejects_bad_host_manifest_digest_syntax() {
    let fixture = installed_fixture();
    mutate_json(&host_manifest_path(&fixture), |value| {
        value["binaries"][0]["sha256"] = serde_json::json!("not-a-digest");
    });

    let report = fixture.resolve(None);

    assert_eq!(report.state, InstallStateKind::InvalidInstallMetadata);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == InstallStateDiagnosticCode::InstallMetadataInvalid
            && diagnostic.field == Some("host_binaries_manifest")
            && diagnostic.message.contains("64-hex sha256")
    }));
}

#[test]
fn resolver_reports_tampered_proof_cache() {
    let fixture = installed_fixture();
    fs::write(
        proof_cache_dir(&fixture).join("m80-release-integrity.json"),
        "tampered\n",
    )
    .expect("tamper proof-cache file");

    let report = fixture.resolve(None);

    assert_eq!(report.state, InstallStateKind::TamperedProofCache);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == InstallStateDiagnosticCode::ProofCacheStale));
}

#[test]
fn resolver_reports_tampered_proof_cache_for_missing_reference() {
    let fixture = installed_fixture();
    fs::remove_file(proof_cache_dir(&fixture).join("SHA256SUMS"))
        .expect("remove proof-cache reference");

    let report = fixture.resolve(None);

    assert_eq!(report.state, InstallStateKind::TamperedProofCache);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == InstallStateDiagnosticCode::ProofCacheStale));
}

#[test]
fn resolver_rejects_proof_cache_symlink_even_when_digest_matches() {
    let fixture = installed_fixture();
    let installed = proof_cache_dir(&fixture).join("m80-release-integrity.json");
    let outside = fixture.temp.path().join("outside-integrity.json");
    fs::copy(&installed, &outside).expect("copy proof-cache file outside version dir");
    fs::remove_file(&installed).expect("remove installed proof-cache file");
    symlink(&outside, &installed).expect("replace proof-cache file with symlink");

    let report = fixture.resolve(None);

    assert_eq!(report.state, InstallStateKind::TamperedProofCache);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == InstallStateDiagnosticCode::ProofCacheStale
            && diagnostic.message.contains("must not traverse symlink")
    }));
}

pub(super) fn write_complete_install_metadata(fixture: &InstallStateFixture, tag: &str) {
    let version_dir = fixture.install_root().join("versions").join(tag);
    let artifacts = version_dir.join("artifacts");
    let bin = version_dir.join("bin");
    fs::create_dir_all(&artifacts).expect("create artifacts dir");
    fs::create_dir_all(&bin).expect("create bin dir");

    let bundle_files = [
        "bin/m80",
        "bin/m80-jailer-harden",
        "bin/m80-net-helper",
        "artifacts/vmlinux",
        "artifacts/output.ext4",
        "artifacts/output.ext4.manifest.json",
        "artifacts/output.ext4.build-receipt.json",
        "artifacts/m80-guestd",
        "install.sh",
    ];
    let files = bundle_files
        .iter()
        .map(|relative| {
            let path = version_dir.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create bundle file parent");
            }
            fs::write(&path, format!("{relative} for {tag}\n")).expect("write bundle file");
            let bytes = fs::read(&path).expect("read bundle file");
            serde_json::json!({
                "path": relative,
                "sha256": sha256_bytes(&bytes),
                "size_bytes": bytes.len(),
            })
        })
        .collect::<Vec<_>>();
    fs::write(
        version_dir.join("bundle.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "release_tag": tag,
            "m80_version": tag,
            "package_version": tag,
            "target": "linux-x86_64",
            "os": "linux",
            "arch": "x86_64",
            "image_kind": "minimal",
            "m80_protocol_version": 1,
            "guestd_package_version": tag,
            "guest_protocol_version": 1,
            "manifest_schema_version": 1,
            "build_receipt_schema_version": 1,
            "build_receipt_manifest_path": "artifacts/output.ext4.manifest.json",
            "install_provenance_schema_version": 1,
            "install_provenance_required": true,
            "expected_firecracker_version": "v1.15.1",
            "files": files,
        }))
        .expect("encode bundle metadata"),
    )
    .expect("write bundle metadata");

    let guest_manifest = artifacts.join("output.ext4.manifest.json");
    let build_receipt = artifacts.join("output.ext4.build-receipt.json");
    InstallProvenance::new(
        Some(tag.to_owned()),
        vec![
            install_transform(
                InstallProvenanceArtifact::GuestManifest,
                "artifacts/output.ext4.manifest.json",
                &guest_manifest,
            ),
            install_transform(
                InstallProvenanceArtifact::BuildReceipt,
                "artifacts/output.ext4.build-receipt.json",
                &build_receipt,
            ),
        ],
    )
    .write(&artifacts.join("install-provenance.json"))
    .expect("write install provenance");

    HostBinariesManifest::new(
        vec![host_binary(
            HostBinaryName::Firecracker,
            "/opt/firecracker/bin/firecracker",
        )],
        vec![host_material(
            HostLaunchMaterialName::FirecrackerSeccompFilter,
            "/opt/firecracker/bin/firecracker-seccomp-filter.bin",
        )],
    )
    .write(&artifacts.join("host-binaries.manifest.json"))
    .expect("write host-binaries manifest");
    proof_fixture::write_proof_cache(&artifacts.join("release-proof-cache"), tag);
}

fn installed_fixture() -> InstallStateFixture {
    let fixture = InstallStateFixture::new();
    fixture.write_installed_profile(TAG);
    write_complete_install_metadata(&fixture, TAG);
    fixture.write_system_config("default");
    fixture.point_active_at(TAG);
    fixture
}

fn version_dir(fixture: &InstallStateFixture) -> PathBuf {
    fixture.install_root().join("versions").join(TAG)
}

fn bundle_path(fixture: &InstallStateFixture) -> PathBuf {
    version_dir(fixture).join("bundle.json")
}

fn install_provenance_path(fixture: &InstallStateFixture) -> PathBuf {
    version_dir(fixture)
        .join("artifacts")
        .join("install-provenance.json")
}

fn host_manifest_path(fixture: &InstallStateFixture) -> PathBuf {
    version_dir(fixture)
        .join("artifacts")
        .join("host-binaries.manifest.json")
}

fn proof_cache_dir(fixture: &InstallStateFixture) -> PathBuf {
    version_dir(fixture)
        .join("artifacts")
        .join("release-proof-cache")
}

fn mutate_json(path: &Path, mutate: impl FnOnce(&mut serde_json::Value)) {
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(path).expect("read mutable metadata JSON"))
            .expect("parse mutable metadata JSON");
    mutate(&mut value);
    fs::write(
        path,
        serde_json::to_vec_pretty(&value).expect("encode mutated metadata JSON"),
    )
    .expect("write mutated metadata JSON");
}

fn install_transform(
    artifact: InstallProvenanceArtifact,
    source_path: &str,
    installed_path: &Path,
) -> InstallProvenanceTransform {
    let bytes = fs::read(installed_path).expect("read install provenance target");
    let digest = sha256_bytes(&bytes);
    InstallProvenanceTransform {
        artifact,
        source_sha256: digest.clone(),
        source_path: PathBuf::from(source_path),
        installed_sha256: digest,
        installed_path: installed_path.to_path_buf(),
        rewrite: InstallProvenanceRewrite::InstallPathRewrite,
    }
}

fn host_binary(name: HostBinaryName, path: &str) -> HostBinaryEntry {
    HostBinaryEntry {
        name,
        path: PathBuf::from(path),
        sha256: "a".repeat(64),
        version: "v1.15.1".to_owned(),
    }
}

fn host_material(name: HostLaunchMaterialName, path: &str) -> HostLaunchMaterialEntry {
    HostLaunchMaterialEntry {
        name,
        path: PathBuf::from(path),
        sha256: "b".repeat(64),
        version: "v1.15.1".to_owned(),
    }
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
