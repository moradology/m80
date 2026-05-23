use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use m80_firecracker::{ConfigError, FcError};
use m80_image_manifest::{
    BuildReceipt, BuildReceiptArtifactKind, InstallProvenance, InstallProvenanceArtifact,
    InstallProvenanceRewrite, InstallProvenanceTransform, Manifest,
};
use serde::Deserialize;

use super::bundle::{sha256_file, PAYLOAD_FILES};

pub(super) const INSTALL_PROVENANCE_FILE: &str = "install-provenance.json";

pub(in crate::cmds::install::layout) fn read_bundle_metadata(
    path: &Path,
) -> Result<BundleMetadata, FcError> {
    let raw = fs::read(path).map_err(|source| FcError::PathIo {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&raw).map_err(|source| FcError::Json {
        context: "read bundle metadata",
        source,
    })
}

pub(super) fn verify_bundle_metadata(metadata: &BundleMetadata) -> Result<(), FcError> {
    if metadata.schema_version != 1 {
        return Err(invalid_bundle(
            "bundle.schema_version",
            format!(
                "unsupported bundle schema_version: {}",
                metadata.schema_version
            ),
        ));
    }
    if metadata.target != "linux-x86_64" || metadata.os != "linux" || metadata.arch != "x86_64" {
        return Err(invalid_bundle(
            "bundle.target",
            format!(
                "unsupported bundle target tuple: target={} os={} arch={}",
                metadata.target, metadata.os, metadata.arch
            ),
        ));
    }
    if metadata.image_kind != "minimal" {
        return Err(invalid_bundle(
            "bundle.image_kind",
            format!("unsupported bundle image_kind: {}", metadata.image_kind),
        ));
    }
    if metadata.m80_protocol_version != m80_proto::PROTOCOL_VERSION
        || metadata.guest_protocol_version != m80_proto::PROTOCOL_VERSION
    {
        return Err(invalid_bundle(
            "bundle.protocol_version",
            format!(
                "guest protocol mismatch: expected_protocol={} actual_m80_protocol={} actual_guest_protocol={} guestd_identity={} rootfs_identity={} running_m80_version={} release_tag={} repair: {}",
                m80_proto::PROTOCOL_VERSION,
                metadata.m80_protocol_version,
                metadata.guest_protocol_version,
                guestd_identity(metadata),
                rootfs_identity(metadata),
                env!("CARGO_PKG_VERSION"),
                metadata.release_tag,
                pinned_reinstall_command(&metadata.release_tag)
            ),
        ));
    }
    if !metadata.install_provenance_required {
        return Err(invalid_bundle(
            "bundle.install_provenance_required",
            "bundle must require installed provenance".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn verify_metadata_hashes(
    root: &Path,
    metadata: &BundleMetadata,
) -> Result<(), FcError> {
    let files = metadata
        .files
        .iter()
        .map(|file| (file.path.as_str(), (file.sha256.as_str(), file.size_bytes)))
        .collect::<BTreeMap<_, _>>();
    for required in PAYLOAD_FILES {
        let (expected_sha, expected_size) = files.get(required).ok_or_else(|| {
            FcError::Config(ConfigError::InvalidValue {
                field: "bundle.files",
                reason: format!("bundle metadata missing file row: {required}"),
            })
        })?;
        let actual_size = root
            .join(required)
            .metadata()
            .map_err(|source| FcError::PathIo {
                path: root.join(required),
                source,
            })?
            .len();
        if actual_size != *expected_size {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "bundle.files",
                reason: format!(
                    "bundle metadata size mismatch for {required}: expected {expected_size}, got {actual_size}"
                ),
            }));
        }
        let actual = sha256_file(&root.join(required))?;
        if actual != *expected_sha {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "bundle.files",
                reason: format!(
                    "bundle metadata hash mismatch for {required}: expected {expected_sha}, got {actual}"
                ),
            }));
        }
    }
    Ok(())
}

pub(super) fn verify_sha256s_file(root: &Path) -> Result<(), FcError> {
    let sums_path = root.join("SHA256SUMS");
    let contents = fs::read_to_string(&sums_path).map_err(|source| FcError::PathIo {
        path: sums_path.clone(),
        source,
    })?;
    let sums = parse_sha256s(&contents)?;
    let expected = PAYLOAD_FILES
        .iter()
        .copied()
        .chain(std::iter::once("bundle.json"))
        .collect::<BTreeSet<_>>();
    let actual = sums.keys().copied().collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(invalid_bundle(
            "SHA256SUMS",
            "SHA256SUMS file set mismatch".to_owned(),
        ));
    }
    for path in expected {
        let expected_sha = sums.get(path).expect("expected path exists in sums");
        let actual_sha = sha256_file(&root.join(path))?;
        if expected_sha != &actual_sha {
            return Err(invalid_bundle(
                "SHA256SUMS",
                format!(
                    "SHA256SUMS hash mismatch for {path}: expected {expected_sha}, got {actual_sha}"
                ),
            ));
        }
    }
    Ok(())
}

fn parse_sha256s(contents: &str) -> Result<BTreeMap<&str, &str>, FcError> {
    let mut sums = BTreeMap::new();
    for (line_no, line) in contents.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut fields = line.split_whitespace();
        let Some(sha256) = fields.next() else {
            continue;
        };
        let Some(path) = fields.next() else {
            return Err(invalid_bundle(
                "SHA256SUMS",
                format!("malformed SHA256SUMS line {}", line_no + 1),
            ));
        };
        if fields.next().is_some()
            || sha256.len() != 64
            || !sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(invalid_bundle(
                "SHA256SUMS",
                format!("malformed SHA256SUMS line {}", line_no + 1),
            ));
        }
        if sums.insert(path, sha256).is_some() {
            return Err(invalid_bundle(
                "SHA256SUMS",
                format!("SHA256SUMS duplicate path: {path}"),
            ));
        }
    }
    Ok(sums)
}

pub(in crate::cmds::install::layout) fn rewrite_installed_metadata(
    root: &Path,
    final_dir: &Path,
    metadata: &BundleMetadata,
) -> Result<(), FcError> {
    let artifacts = final_dir.join("artifacts");
    let manifest_path = root.join("artifacts/output.ext4.manifest.json");
    let receipt_path = root.join("artifacts/output.ext4.build-receipt.json");
    let source_manifest_sha = sha256_file(&manifest_path)?;
    let source_receipt_sha = sha256_file(&receipt_path)?;

    let mut manifest = Manifest::read(&manifest_path).map_err(FcError::Manifest)?;
    if manifest.schema_version() != metadata.manifest_schema_version {
        return Err(invalid_bundle(
            "bundle.manifest_schema_version",
            format!(
                "manifest schema mismatch: expected_schema={} actual_schema={} manifest_path={} running_m80_version={} selected_install_profile=default release_tag={} repair: {}",
                metadata.manifest_schema_version,
                manifest.schema_version(),
                manifest_path.display(),
                env!("CARGO_PKG_VERSION"),
                metadata.release_tag,
                pinned_reinstall_command(&metadata.release_tag)
            ),
        ));
    }
    if manifest.expected_firecracker_version != metadata.expected_firecracker_version {
        return Err(invalid_bundle(
            "bundle.expected_firecracker_version",
            "bundle expected_firecracker_version mismatch".to_owned(),
        ));
    }
    manifest.daemon_binary_path = artifacts.join("m80-guestd");
    manifest.kernel_image = artifacts.join("vmlinux");
    manifest.output_rootfs_image = artifacts.join("output.ext4");
    manifest.write(&manifest_path).map_err(FcError::Manifest)?;
    let installed_manifest_sha = sha256_file(&manifest_path)?;

    let mut receipt = BuildReceipt::read(&receipt_path).map_err(FcError::Manifest)?;
    if receipt.schema_version() != metadata.build_receipt_schema_version {
        return Err(invalid_bundle(
            "bundle.build_receipt_schema_version",
            format!(
                "bundle build_receipt_schema_version mismatch: expected {}, got {}",
                metadata.build_receipt_schema_version,
                receipt.schema_version()
            ),
        ));
    }
    if receipt.manifest_path != PathBuf::from(&metadata.build_receipt_manifest_path) {
        return Err(invalid_bundle(
            "bundle.build_receipt_manifest_path",
            "bundle build_receipt_manifest_path mismatch".to_owned(),
        ));
    }
    receipt.manifest_path = artifacts.join("output.ext4.manifest.json");
    receipt.manifest_sha256 = installed_manifest_sha.clone();
    for artifact in &mut receipt.artifacts {
        artifact.path = match artifact.kind {
            BuildReceiptArtifactKind::KernelImage => artifacts.join("vmlinux"),
            BuildReceiptArtifactKind::OutputRootfsImage => artifacts.join("output.ext4"),
            BuildReceiptArtifactKind::DaemonBinaryPath => artifacts.join("m80-guestd"),
            BuildReceiptArtifactKind::SourceRootfsImage => artifact.path.clone(),
        };
    }
    receipt.write(&receipt_path).map_err(FcError::Manifest)?;
    let installed_receipt_sha = sha256_file(&receipt_path)?;

    let provenance = InstallProvenance::new(
        Some(metadata.release_tag.clone()),
        vec![
            install_path_rewrite_transform(
                InstallProvenanceArtifact::GuestManifest,
                "artifacts/output.ext4.manifest.json",
                artifacts.join("output.ext4.manifest.json"),
                source_manifest_sha,
                installed_manifest_sha,
            ),
            install_path_rewrite_transform(
                InstallProvenanceArtifact::BuildReceipt,
                "artifacts/output.ext4.build-receipt.json",
                artifacts.join("output.ext4.build-receipt.json"),
                source_receipt_sha,
                installed_receipt_sha,
            ),
        ],
    );
    provenance
        .write(&root.join("artifacts").join(INSTALL_PROVENANCE_FILE))
        .map_err(FcError::Manifest)?;

    rewrite_installed_bundle_metadata(
        root,
        final_dir,
        &[
            "artifacts/output.ext4.manifest.json",
            "artifacts/output.ext4.build-receipt.json",
        ],
    )?;
    rewrite_installed_sha256s(root)
}

fn install_path_rewrite_transform(
    artifact: InstallProvenanceArtifact,
    source_path: &str,
    installed_path: PathBuf,
    source_sha256: String,
    installed_sha256: String,
) -> InstallProvenanceTransform {
    InstallProvenanceTransform {
        artifact,
        source_sha256,
        source_path: PathBuf::from(source_path),
        installed_sha256,
        installed_path,
        rewrite: InstallProvenanceRewrite::InstallPathRewrite,
    }
}

fn rewrite_installed_bundle_metadata(
    root: &Path,
    final_dir: &Path,
    rewritten_files: &[&str],
) -> Result<(), FcError> {
    let bundle_path = root.join("bundle.json");
    let raw = fs::read(&bundle_path).map_err(|source| FcError::PathIo {
        path: bundle_path.clone(),
        source,
    })?;
    let mut value: serde_json::Value =
        serde_json::from_slice(&raw).map_err(|source| FcError::Json {
            context: "rewrite installed bundle metadata",
            source,
        })?;
    value["build_receipt_manifest_path"] = serde_json::Value::String(
        final_dir
            .join("artifacts/output.ext4.manifest.json")
            .display()
            .to_string(),
    );
    for relative in rewritten_files {
        rewrite_bundle_file_record(root, &mut value, relative)?;
    }
    let mut encoded = serde_json::to_vec_pretty(&value).map_err(|source| FcError::Json {
        context: "encode installed bundle metadata",
        source,
    })?;
    encoded.push(b'\n');
    fs::write(&bundle_path, encoded).map_err(|source| FcError::PathIo {
        path: bundle_path,
        source,
    })
}

fn rewrite_bundle_file_record(
    root: &Path,
    metadata: &mut serde_json::Value,
    relative: &str,
) -> Result<(), FcError> {
    let path = root.join(relative);
    let size_bytes = path
        .metadata()
        .map_err(|source| FcError::PathIo {
            path: path.clone(),
            source,
        })?
        .len();
    let sha256 = sha256_file(&path)?;
    let files = metadata["files"].as_array_mut().ok_or_else(|| {
        FcError::Config(ConfigError::InvalidValue {
            field: "bundle.files",
            reason: "bundle metadata files must be an array".to_owned(),
        })
    })?;
    let Some(record) = files.iter_mut().find(|record| {
        record
            .get("path")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|path| path == relative)
    }) else {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "bundle.files",
            reason: format!("bundle metadata missing file row: {relative}"),
        }));
    };
    record["sha256"] = serde_json::Value::String(sha256);
    record["size_bytes"] = serde_json::Value::Number(size_bytes.into());
    Ok(())
}

fn rewrite_installed_sha256s(root: &Path) -> Result<(), FcError> {
    let mut lines = String::new();
    for relative in PAYLOAD_FILES.iter().copied().chain(["bundle.json"]) {
        lines.push_str(&format!(
            "{}  {relative}\n",
            sha256_file(&root.join(relative))?
        ));
    }
    let path = root.join("SHA256SUMS");
    fs::write(&path, lines).map_err(|source| FcError::PathIo { path, source })
}

pub(in crate::cmds::install::layout) fn set_final_modes(root: &Path) -> Result<(), FcError> {
    for path in [
        "bin/m80",
        "bin/m80-jailer-harden",
        "bin/m80-net-helper",
        "install.sh",
    ] {
        set_mode(&root.join(path), 0o755)?;
    }
    for path in [
        "artifacts/vmlinux",
        "artifacts/output.ext4",
        "artifacts/output.ext4.manifest.json",
        "artifacts/output.ext4.build-receipt.json",
        "artifacts/m80-guestd",
        "artifacts/install-provenance.json",
        "bundle.json",
        "SHA256SUMS",
    ] {
        set_mode(&root.join(path), 0o644)?;
    }
    Ok(())
}

fn set_mode(path: &Path, mode: u32) -> Result<(), FcError> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|source| FcError::PathIo {
        path: path.to_path_buf(),
        source,
    })
}

fn invalid_bundle(field: &'static str, reason: String) -> FcError {
    FcError::Config(ConfigError::InvalidValue { field, reason })
}

fn pinned_reinstall_command(release_tag: &str) -> String {
    format!(
        "curl -fsSL {} | sudo sh",
        crate::release_urls::release_install_url(release_tag)
    )
}

fn guestd_identity(metadata: &BundleMetadata) -> String {
    format!(
        "package_version={} sha256={}",
        metadata.guestd_package_version,
        bundle_file_sha256(metadata, "artifacts/m80-guestd")
    )
}

fn rootfs_identity(metadata: &BundleMetadata) -> String {
    format!(
        "image_kind={} sha256={}",
        metadata.image_kind,
        bundle_file_sha256(metadata, "artifacts/output.ext4")
    )
}

fn bundle_file_sha256<'a>(metadata: &'a BundleMetadata, path: &str) -> &'a str {
    metadata
        .files
        .iter()
        .find(|file| file.path == path)
        .map(|file| file.sha256.as_str())
        .unwrap_or("<missing>")
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BundleMetadata {
    schema_version: u32,
    pub(super) release_tag: String,
    m80_version: String,
    package_version: String,
    target: String,
    os: String,
    arch: String,
    image_kind: String,
    m80_protocol_version: u32,
    guestd_package_version: String,
    guest_protocol_version: u32,
    manifest_schema_version: u32,
    build_receipt_schema_version: u32,
    build_receipt_manifest_path: String,
    install_provenance_schema_version: u32,
    install_provenance_required: bool,
    pub(super) expected_firecracker_version: String,
    files: Vec<BundleFile>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleFile {
    path: String,
    sha256: String,
    size_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_mismatch_names_expected_actual_guest_rootfs_and_repair() {
        let mut metadata = fixture_metadata();
        metadata.guest_protocol_version = m80_proto::PROTOCOL_VERSION + 1;

        let err = verify_bundle_metadata(&metadata).unwrap_err().to_string();

        assert!(err.contains("guest protocol mismatch"), "{err}");
        assert!(
            err.contains(&format!(
                "expected_protocol={}",
                m80_proto::PROTOCOL_VERSION
            )),
            "{err}"
        );
        assert!(
            err.contains(&format!(
                "actual_guest_protocol={}",
                m80_proto::PROTOCOL_VERSION + 1
            )),
            "{err}"
        );
        assert!(
            err.contains("guestd_identity=package_version=0.2.0"),
            "{err}"
        );
        assert!(err.contains("rootfs_identity=image_kind=minimal"), "{err}");
        assert!(
            err.contains("running_m80_version=") && err.contains("release_tag=v0.2.0"),
            "{err}"
        );
        assert!(
            err.contains(
                "repair: curl -fsSL https://github.com/moradology/m80/releases/download/v0.2.0/install.sh | sudo sh"
            ),
            "{err}"
        );
    }

    #[test]
    fn manifest_schema_mismatch_names_path_profile_versions_and_repair() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let artifacts = root.join("artifacts");
        std::fs::create_dir_all(&artifacts).unwrap();
        let manifest_path = artifacts.join("output.ext4.manifest.json");
        std::fs::write(
            &manifest_path,
            format!(
                r#"{{
  "daemon_binary_path": "m80-guestd",
  "daemon_binary_sha256": "{}",
  "expected_firecracker_version": "v1.15.1",
  "guest_port": 52,
  "image_kind": "minimal",
  "kernel_image": "vmlinux",
  "kernel_image_sha256": "{}",
  "kernel_kind": "stripped",
  "no_egress_reason": null,
  "output_rootfs_image": "output.ext4",
  "output_rootfs_sha256": "{}",
  "ready_marker": "M80_READY",
  "rootfs_format": "ext4",
  "schema_version": {},
  "source_rootfs_image": null,
  "source_rootfs_sha256": null
}}"#,
                "0".repeat(64),
                "1".repeat(64),
                "2".repeat(64),
                m80_image_manifest::SCHEMA_VERSION
            ),
        )
        .unwrap();
        std::fs::write(artifacts.join("output.ext4.build-receipt.json"), "{}").unwrap();
        let mut metadata = fixture_metadata();
        metadata.manifest_schema_version = m80_image_manifest::SCHEMA_VERSION + 1;

        let err = rewrite_installed_metadata(root, &root.join("versions/v0.2.0"), &metadata)
            .unwrap_err()
            .to_string();

        assert!(err.contains("manifest schema mismatch"), "{err}");
        assert!(
            err.contains(&format!(
                "expected_schema={}",
                m80_image_manifest::SCHEMA_VERSION + 1
            )),
            "{err}"
        );
        assert!(
            err.contains(&format!(
                "actual_schema={}",
                m80_image_manifest::SCHEMA_VERSION
            )),
            "{err}"
        );
        assert!(
            err.contains(&format!("manifest_path={}", manifest_path.display())),
            "{err}"
        );
        assert!(err.contains("running_m80_version="), "{err}");
        assert!(err.contains("selected_install_profile=default"), "{err}");
        assert!(
            err.contains("/releases/download/v0.2.0/install.sh | sudo sh"),
            "{err}"
        );
    }

    fn fixture_metadata() -> BundleMetadata {
        BundleMetadata {
            schema_version: 1,
            release_tag: "v0.2.0".to_owned(),
            m80_version: "0.2.0".to_owned(),
            package_version: "0.2.0".to_owned(),
            target: "linux-x86_64".to_owned(),
            os: "linux".to_owned(),
            arch: "x86_64".to_owned(),
            image_kind: "minimal".to_owned(),
            m80_protocol_version: m80_proto::PROTOCOL_VERSION,
            guestd_package_version: "0.2.0".to_owned(),
            guest_protocol_version: m80_proto::PROTOCOL_VERSION,
            manifest_schema_version: m80_image_manifest::SCHEMA_VERSION,
            build_receipt_schema_version: m80_image_manifest::BUILD_RECEIPT_SCHEMA_VERSION,
            build_receipt_manifest_path: "artifacts/output.ext4.manifest.json".to_owned(),
            install_provenance_schema_version:
                m80_image_manifest::INSTALL_PROVENANCE_SCHEMA_VERSION,
            install_provenance_required: true,
            expected_firecracker_version: "v1.15.1".to_owned(),
            files: vec![
                BundleFile {
                    path: "artifacts/m80-guestd".to_owned(),
                    sha256: "a".repeat(64),
                    size_bytes: 1,
                },
                BundleFile {
                    path: "artifacts/output.ext4".to_owned(),
                    sha256: "b".repeat(64),
                    size_bytes: 1,
                },
            ],
        }
    }
}
