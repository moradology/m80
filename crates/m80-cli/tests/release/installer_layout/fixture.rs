use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use m80_image_manifest::{
    BuildReceipt, BuildReceiptArtifact, BuildReceiptArtifactKind, ImageKind, KernelKind, Manifest,
    RootfsFormat, BUILD_RECEIPT_SCHEMA_VERSION, INSTALL_PROVENANCE_SCHEMA_VERSION,
};
use serde_json::json;
use sha2::{Digest, Sha256};

pub(crate) const RELEASE_TAG: &str = concat!("v", env!("CARGO_PKG_VERSION"));
const REQUIRED_BUNDLE_FILES: &[&str] = &[
    "bin/m80",
    "bin/m80-jailer-harden",
    "bin/m80-net-helper",
    "artifacts/vmlinux",
    "artifacts/output.ext4",
    "artifacts/output.ext4.manifest.json",
    "artifacts/output.ext4.build-receipt.json",
    "artifacts/m80-guestd",
    "install.sh",
    "bundle.json",
    "SHA256SUMS",
];
const PAYLOAD_FILES: &[&str] = &[
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

pub(crate) struct ReleaseBundleFixture {
    _temp: tempfile::TempDir,
    pub(crate) tarball: PathBuf,
    pub(crate) release_tag: String,
}

pub(crate) fn write_release_bundle(omit: Option<&str>) -> ReleaseBundleFixture {
    write_release_bundle_with_hook(omit, |_| {})
}

pub(crate) fn write_release_bundle_with_hook<F>(omit: Option<&str>, hook: F) -> ReleaseBundleFixture
where
    F: FnOnce(&Path),
{
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("src");
    fs::create_dir_all(src.join("bin")).unwrap();
    fs::create_dir_all(src.join("artifacts")).unwrap();
    write_executable(
        &src.join("bin/m80"),
        &format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then printf 'm80 {RELEASE_TAG}\\n'; exit 0; fi\nprintf 'm80 fixture\\n'\n"
        ),
    );
    write_executable(
        &src.join("bin/m80-jailer-harden"),
        &format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then printf 'm80-jailer-harden {RELEASE_TAG}\\n'; exit 0; fi\n"
        ),
    );
    write_executable(
        &src.join("bin/m80-net-helper"),
        &format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then printf 'm80-net-helper {RELEASE_TAG}\\n'; exit 0; fi\n"
        ),
    );
    fs::write(src.join("artifacts/vmlinux"), b"kernel").unwrap();
    fs::write(src.join("artifacts/output.ext4"), b"rootfs").unwrap();
    fs::write(src.join("artifacts/m80-guestd"), b"guestd").unwrap();
    fs::write(
        src.join("install.sh"),
        b"#!/bin/sh\nexec ./bin/m80 install \"$@\"\n",
    )
    .unwrap();

    let stale_artifacts = PathBuf::from("/tmp/m80-release-bundle/artifacts");
    write_manifest(&src, &stale_artifacts);
    let manifest_path = stale_artifacts.join("output.ext4.manifest.json");
    write_build_receipt(&src, &stale_artifacts, &manifest_path);
    write_bundle_metadata(&src, &manifest_path);
    write_sha256s(&src);
    set_bundle_modes(&src);
    hook(&src);

    let tarball = temp.path().join("m80-linux-x86_64.tar.gz");
    let paths = REQUIRED_BUNDLE_FILES
        .iter()
        .copied()
        .filter(|path| Some(*path) != omit)
        .collect::<Vec<_>>();
    run_checked(
        StdCommand::new("tar")
            .arg("-czf")
            .arg(&tarball)
            .arg("-C")
            .arg(&src)
            .args(paths),
        "tar",
    );

    ReleaseBundleFixture {
        _temp: temp,
        tarball,
        release_tag: RELEASE_TAG.to_owned(),
    }
}

fn write_manifest(src: &Path, stale_artifacts: &Path) {
    let manifest = Manifest::new(
        stale_artifacts.join("m80-guestd"),
        sha256_hex(&src.join("artifacts/m80-guestd")),
        "v1.15.1".to_owned(),
        m80_proto::GUEST_PORT_DEFAULT,
        ImageKind::Minimal,
        stale_artifacts.join("vmlinux"),
        sha256_hex(&src.join("artifacts/vmlinux")),
        KernelKind::Stock,
        Some(m80_image_manifest::DEFAULT_NO_EGRESS_REASON.to_owned()),
        stale_artifacts.join("output.ext4"),
        sha256_hex(&src.join("artifacts/output.ext4")),
        m80_proto::READY_MARKER_DEFAULT.to_owned(),
        RootfsFormat::Ext4,
        None,
        None,
    );
    manifest
        .write(&src.join("artifacts/output.ext4.manifest.json"))
        .unwrap();
}

fn write_build_receipt(src: &Path, stale_artifacts: &Path, manifest_path: &Path) {
    let receipt = BuildReceipt::new(
        manifest_path.to_path_buf(),
        sha256_hex(&src.join("artifacts/output.ext4.manifest.json")),
        vec![
            BuildReceiptArtifact {
                kind: BuildReceiptArtifactKind::KernelImage,
                path: stale_artifacts.join("vmlinux"),
                sha256: sha256_hex(&src.join("artifacts/vmlinux")),
            },
            BuildReceiptArtifact {
                kind: BuildReceiptArtifactKind::OutputRootfsImage,
                path: stale_artifacts.join("output.ext4"),
                sha256: sha256_hex(&src.join("artifacts/output.ext4")),
            },
            BuildReceiptArtifact {
                kind: BuildReceiptArtifactKind::DaemonBinaryPath,
                path: stale_artifacts.join("m80-guestd"),
                sha256: sha256_hex(&src.join("artifacts/m80-guestd")),
            },
        ],
    );
    receipt
        .write(&src.join("artifacts/output.ext4.build-receipt.json"))
        .unwrap();
}

fn write_bundle_metadata(src: &Path, manifest_path: &Path) {
    let files = PAYLOAD_FILES
        .iter()
        .map(|path| {
            let full = src.join(path);
            json!({
                "path": path,
                "sha256": sha256_hex(&full),
                "size_bytes": full.metadata().unwrap().len(),
            })
        })
        .collect::<Vec<_>>();
    let metadata = json!({
        "schema_version": 1,
        "release_tag": RELEASE_TAG,
        "m80_version": RELEASE_TAG,
        "package_version": env!("CARGO_PKG_VERSION"),
        "target": "linux-x86_64",
        "os": "linux",
        "arch": "x86_64",
        "image_kind": "minimal",
        "m80_protocol_version": m80_proto::PROTOCOL_VERSION,
        "guestd_package_version": env!("CARGO_PKG_VERSION"),
        "guest_protocol_version": m80_proto::PROTOCOL_VERSION,
        "manifest_schema_version": m80_image_manifest::SCHEMA_VERSION,
        "build_receipt_schema_version": BUILD_RECEIPT_SCHEMA_VERSION,
        "build_receipt_manifest_path": manifest_path.display().to_string(),
        "install_provenance_schema_version": INSTALL_PROVENANCE_SCHEMA_VERSION,
        "install_provenance_required": true,
        "expected_firecracker_version": "v1.15.1",
        "files": files,
    });
    let mut encoded = serde_json::to_string_pretty(&metadata).unwrap();
    encoded.push('\n');
    fs::write(src.join("bundle.json"), encoded).unwrap();
}

fn write_sha256s(src: &Path) {
    let mut lines = String::new();
    for relpath in PAYLOAD_FILES.iter().copied().chain(["bundle.json"]) {
        lines.push_str(&format!("{}  {relpath}\n", sha256_hex(&src.join(relpath))));
    }
    fs::write(src.join("SHA256SUMS"), lines).unwrap();
}

pub(crate) fn write_duplicate_path_bundle(root: &Path) -> PathBuf {
    let src = root.join("dup-src");
    fs::create_dir_all(src.join("bin")).unwrap();
    fs::write(src.join("bin/m80"), b"duplicate").unwrap();
    let tarball = root.join("duplicate.tar.gz");
    run_checked(
        StdCommand::new("tar")
            .arg("-czf")
            .arg(&tarball)
            .arg("-C")
            .arg(&src)
            .arg("bin/m80")
            .arg("-C")
            .arg(&src)
            .arg("bin/m80"),
        "duplicate tar",
    );
    tarball
}

pub(crate) fn sha256_hex(path: &Path) -> String {
    let bytes = fs::read(path).unwrap();
    format!("{:x}", Sha256::digest(&bytes))
}

fn run_checked(cmd: &mut StdCommand, label: &str) {
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn set_bundle_modes(src: &Path) {
    for relpath in [
        "bin/m80",
        "bin/m80-jailer-harden",
        "bin/m80-net-helper",
        "install.sh",
    ] {
        set_mode(&src.join(relpath), 0o755);
    }
    for relpath in [
        "artifacts/vmlinux",
        "artifacts/output.ext4",
        "artifacts/output.ext4.manifest.json",
        "artifacts/output.ext4.build-receipt.json",
        "artifacts/m80-guestd",
        "bundle.json",
        "SHA256SUMS",
    ] {
        set_mode(&src.join(relpath), 0o644);
    }
}

pub(crate) fn set_mode(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

pub(crate) fn running_as_root() -> bool {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find(|line| line.starts_with("Uid:"))
                .and_then(|line| line.split_whitespace().nth(1))
                .and_then(|uid| uid.parse::<u32>().ok())
        })
        == Some(0)
}

pub(crate) fn read_repo_file(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative)).expect("read repository file")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("m80-cli crate should be under crates/")
        .to_path_buf()
}
