use std::io::Write as _;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use m80_image_store::{ImageArtifact, ImageDigest, ImageKind, ImageStore, StoreError};
use sha2::{Digest as _, Sha256};

fn open_temp_store() -> (tempfile::TempDir, ImageStore) {
    let root = tempfile::tempdir().expect("store root");
    let store = ImageStore::open(root.path()).expect("open store");
    (root, store)
}

fn write_file(path: &Path, bytes: &[u8]) {
    let mut file = std::fs::File::create(path).expect("create file");
    file.write_all(bytes).expect("write file");
    file.sync_all().expect("sync file");
}

fn source_file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    write_file(&path, bytes);
    path
}

fn digest_for(bytes: &[u8]) -> ImageDigest {
    ImageDigest::parse(&hex::encode(Sha256::digest(bytes))).expect("digest")
}

fn stored_path(root: &Path, digest: &ImageDigest, kind: ImageKind) -> PathBuf {
    root.join(&digest.as_str()[0..2])
        .join(digest.as_str())
        .join(kind.file_name())
}

fn source_tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("source tree");
    std::fs::create_dir(dir.path().join("bin")).expect("bin dir");
    write_file(&dir.path().join("bin/tool"), b"#!/bin/sh\nexit 0\n");
    write_file(&dir.path().join("README"), b"fixture\n");
    dir
}

fn erofs_source(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    build_erofs_source(dir, name, bytes, &[])
}

fn build_erofs_source(dir: &Path, name: &str, bytes: &[u8], extra_args: &[&str]) -> PathBuf {
    let source_dir = dir.join(format!("{name}.src"));
    std::fs::create_dir(&source_dir).expect("erofs source dir");
    write_file(&source_dir.join("payload.bin"), bytes);
    let output = dir.join(name);
    let mut command = Command::new("mkfs.erofs");
    command
        .arg("--quiet")
        .arg("-T")
        .arg("0")
        .arg("--all-time")
        .arg("--all-root")
        .arg("--force-uid=0")
        .arg("--force-gid=0")
        .args(extra_args)
        .arg(&output)
        .arg(&source_dir);
    let result = command.output().expect("spawn mkfs.erofs");
    assert!(
        result.status.success(),
        "mkfs.erofs failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    output
}

#[test]
fn open_rejects_relative_root() {
    let err = ImageStore::open(Path::new("relative-store")).expect_err("relative root must fail");

    assert!(
        matches!(
            err,
            StoreError::InvalidPath {
                reason: "store root must be absolute",
                ..
            }
        ),
        "got {err:?}"
    );
}

#[test]
fn open_rejects_symlink_root() {
    let parent = tempfile::tempdir().expect("parent");
    let real = parent.path().join("real");
    let link = parent.path().join("link");
    std::fs::create_dir(&real).expect("real dir");
    symlink(&real, &link).expect("symlink root");

    let err = ImageStore::open(&link).expect_err("symlink root must fail");

    assert!(
        matches!(
            err,
            StoreError::InvalidPath {
                reason: "store root must not be a symlink",
                ..
            }
        ),
        "got {err:?}"
    );
}

#[test]
fn gc_execute_guard_waits_for_template_build_guard() {
    let (root, store) = open_temp_store();
    let build_guard = store
        .acquire_template_build_guard()
        .expect("template build guard");
    let store_for_thread = store.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let (acquired_tx, acquired_rx) = mpsc::channel();

    let thread = thread::spawn(move || {
        started_tx.send(()).unwrap();
        let _gc_guard = store_for_thread
            .acquire_gc_execute_guard()
            .expect("gc execute guard");
        acquired_tx.send(()).unwrap();
    });

    started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("gc thread started");
    assert!(
        acquired_rx.recv_timeout(Duration::from_millis(50)).is_err(),
        "exclusive GC guard must wait for shared template-build guard"
    );

    drop(build_guard);

    acquired_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("gc guard acquired after build guard drop");
    thread.join().expect("gc thread");
    assert!(root
        .path()
        .join(".image-template-coordination.lock")
        .is_file());
}

#[test]
fn import_existing_erofs_is_content_addressed_and_stable() {
    let (root, store) = open_temp_store();
    let source = erofs_source(root.path(), "input.erofs", b"erofs bytes");

    let first = store
        .import_existing(&source, ImageKind::Erofs)
        .expect("first import");
    let second = store
        .import_existing(&source, ImageKind::Erofs)
        .expect("second import");

    assert_eq!(first, second);
    assert_eq!(
        first,
        digest_for(&std::fs::read(&source).expect("read erofs source"))
    );
    assert_eq!(
        stored_path(root.path(), &first, ImageKind::Erofs),
        root.path()
            .join(&first.as_str()[0..2])
            .join(first.as_str())
            .join("image.erofs")
    );
    store.verify(&first).expect("verify imported erofs");
}

#[test]
fn import_existing_erofs_rejects_non_erofs_bytes() {
    let (root, store) = open_temp_store();
    let source = source_file(root.path(), "not-erofs.img", b"not an erofs filesystem");

    let err = store
        .import_existing(&source, ImageKind::Erofs)
        .expect_err("non-erofs bytes must fail");

    assert!(
        matches!(err, StoreError::ErofsProbeFailed { .. }),
        "got {err:?}"
    );
}

#[test]
fn import_existing_erofs_rejects_unpinned_compressor() {
    let (root, store) = open_temp_store();
    let source = build_erofs_source(
        root.path(),
        "zstd.erofs",
        &vec![0u8; 1024 * 1024],
        &["-zzstd"],
    );

    let err = store
        .import_existing(&source, ImageKind::Erofs)
        .expect_err("zstd erofs must fail");

    assert!(
        matches!(
            err,
            StoreError::UnsupportedErofsCompression { ref algorithm, .. }
                if algorithm == "zstd"
        ),
        "got {err:?}"
    );
}

#[test]
fn import_existing_erofs_rejects_unpinned_feature() {
    let (root, store) = open_temp_store();
    let source = build_erofs_source(
        root.path(),
        "chunked.erofs",
        b"chunked erofs bytes",
        &["--chunksize=4096"],
    );

    let err = store
        .import_existing(&source, ImageKind::Erofs)
        .expect_err("chunked erofs must fail");

    assert!(
        matches!(
            err,
            StoreError::UnsupportedErofsFeature { ref feature, .. }
                if feature == "chunked_file"
        ),
        "got {err:?}"
    );
}

#[test]
fn import_existing_ext4_is_content_addressed_and_stable() {
    let (root, store) = open_temp_store();
    let source = source_file(root.path(), "input.ext4", b"ext4 bytes");

    let first = store
        .import_existing(&source, ImageKind::Ext4)
        .expect("first import");
    let second = store
        .import_existing(&source, ImageKind::Ext4)
        .expect("second import");

    assert_eq!(first, second);
    assert_eq!(first, digest_for(b"ext4 bytes"));
    store.verify(&first).expect("verify imported ext4");
}

#[test]
fn resolve_returns_canonical_erofs_artifact() {
    let (root, store) = open_temp_store();
    let source = erofs_source(root.path(), "input.erofs", b"resolve erofs");
    let source_len = source.metadata().expect("source metadata").len();
    let digest = store
        .import_existing(&source, ImageKind::Erofs)
        .expect("import");

    let resolved = store.resolve(&digest).expect("resolve");

    let ImageArtifact::Erofs(image) = resolved else {
        panic!("expected erofs artifact");
    };
    assert_eq!(image.digest(), &digest);
    assert_eq!(image.size_bytes(), source_len);
    assert_eq!(
        image.path(),
        stored_path(root.path(), &digest, ImageKind::Erofs)
    );
}

#[test]
fn resolve_returns_canonical_ext4_artifact() {
    let (root, store) = open_temp_store();
    let source = source_file(root.path(), "input.ext4", b"resolve ext4");
    let digest = store
        .import_existing(&source, ImageKind::Ext4)
        .expect("import");

    let resolved = store.resolve(&digest).expect("resolve");

    let ImageArtifact::Ext4(image) = resolved else {
        panic!("expected ext4 artifact");
    };
    assert_eq!(image.digest(), &digest);
    assert_eq!(image.size_bytes(), b"resolve ext4".len() as u64);
    assert_eq!(
        image.path(),
        stored_path(root.path(), &digest, ImageKind::Ext4)
    );
}

#[test]
fn resolve_as_selects_kind_when_digest_has_both_kinds() {
    let (root, store) = open_temp_store();
    let source = erofs_source(root.path(), "same-bytes.img", b"kind selection");
    let digest = store
        .import_existing(&source, ImageKind::Erofs)
        .expect("erofs import");
    store
        .import_existing(&source, ImageKind::Ext4)
        .expect("ext4 import");

    let resolved = store
        .resolve_as(&digest, ImageKind::Erofs)
        .expect("resolve erofs");

    assert_eq!(resolved.kind(), ImageKind::Erofs);
    assert_eq!(
        resolved.path(),
        stored_path(root.path(), &digest, ImageKind::Erofs)
    );
}

#[test]
fn resolve_missing_digest_returns_not_found() {
    let (_root, store) = open_temp_store();
    let digest =
        ImageDigest::parse("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
            .expect("digest");

    let err = store
        .resolve(&digest)
        .expect_err("missing digest must fail");

    assert!(matches!(err, StoreError::NotFound { .. }), "got {err:?}");
}

#[test]
fn verify_reports_digest_mismatch_after_tamper() {
    let (root, store) = open_temp_store();
    let source = erofs_source(root.path(), "input.erofs", b"original");
    let digest = store
        .import_existing(&source, ImageKind::Erofs)
        .expect("import");
    write_file(
        &stored_path(root.path(), &digest, ImageKind::Erofs),
        b"tampered",
    );

    let err = store.verify(&digest).expect_err("tamper must fail");

    assert!(
        matches!(err, StoreError::DigestMismatch { .. }),
        "got {err:?}"
    );
}

#[test]
fn import_rejects_symlink_artifact_placeholder() {
    let (root, store) = open_temp_store();
    let source = erofs_source(root.path(), "input.erofs", b"symlink target");
    let digest = digest_for(&std::fs::read(&source).expect("read erofs source"));
    let artifact_dir = root
        .path()
        .join(&digest.as_str()[0..2])
        .join(digest.as_str());
    std::fs::create_dir(root.path().join(&digest.as_str()[0..2])).expect("shard dir");
    std::fs::create_dir(&artifact_dir).expect("artifact dir");
    symlink(
        root.path().join("outside"),
        stored_path(root.path(), &digest, ImageKind::Erofs),
    )
    .expect("artifact symlink");

    let err = store
        .import_existing(&source, ImageKind::Erofs)
        .expect_err("symlink placeholder must fail");

    assert!(
        matches!(
            err,
            StoreError::InvalidPath {
                reason: "artifact path must not be a symlink",
                ..
            }
        ),
        "got {err:?}"
    );
}

#[test]
fn same_digest_can_store_both_kinds_without_overwrite() {
    let (root, store) = open_temp_store();
    let source = erofs_source(root.path(), "same-bytes.img", b"same bytes");

    let digest = store
        .import_existing(&source, ImageKind::Erofs)
        .expect("erofs import");
    assert_eq!(
        digest,
        store
            .import_existing(&source, ImageKind::Ext4)
            .expect("ext4 import")
    );

    assert!(stored_path(root.path(), &digest, ImageKind::Erofs).is_file());
    assert!(stored_path(root.path(), &digest, ImageKind::Ext4).is_file());
    let err = store
        .resolve(&digest)
        .expect_err("same digest with two kinds is ambiguous");
    assert!(
        matches!(err, StoreError::AmbiguousDigest { .. }),
        "got {err:?}"
    );
}

#[test]
fn list_and_describe_report_all_artifact_kinds() {
    let (root, store) = open_temp_store();
    let source_dir = tempfile::tempdir().expect("sources");
    let first_source = erofs_source(source_dir.path(), "first.img", b"first image");
    let first_len = first_source.metadata().expect("first metadata").len();
    let second_source = source_file(source_dir.path(), "second.img", b"second image");

    let first = store
        .import_existing(&first_source, ImageKind::Erofs)
        .expect("first import");
    let second = store
        .import_existing(&second_source, ImageKind::Ext4)
        .expect("second import");

    let listed = store.list().expect("list images");
    assert_eq!(listed.len(), 2);
    let first_record = listed
        .iter()
        .find(|record| record.digest() == &first)
        .expect("first record");
    assert_eq!(first_record.kind(), ImageKind::Erofs);
    assert_eq!(first_record.size_bytes(), first_len);
    let second_record = listed
        .iter()
        .find(|record| record.digest() == &second)
        .expect("second record");
    assert_eq!(second_record.kind(), ImageKind::Ext4);

    let described = store.describe(&first).expect("describe first");
    assert_eq!(described.len(), 1);
    assert_eq!(
        described[0].path(),
        stored_path(root.path(), &first, ImageKind::Erofs)
    );
}

#[test]
fn remove_deletes_digest_artifacts_and_metadata() {
    let (root, store) = open_temp_store();
    let source = erofs_source(root.path(), "remove.img", b"remove image");
    let digest = store
        .import_existing(&source, ImageKind::Erofs)
        .expect("erofs import");
    store
        .import_existing(&source, ImageKind::Ext4)
        .expect("ext4 import");

    let removed = store.remove(&digest).expect("remove digest");

    assert_eq!(removed.len(), 2);
    assert!(
        !root
            .path()
            .join(&digest.as_str()[0..2])
            .join(digest.as_str())
            .exists(),
        "digest directory should be removed"
    );
    assert!(matches!(
        store.describe(&digest),
        Err(StoreError::NotFound { .. })
    ));
}

#[test]
fn remove_refuses_active_shared_refs() {
    let (root, store) = open_temp_store();
    let source = erofs_source(root.path(), "active.img", b"active image");
    let digest = store
        .import_existing(&source, ImageKind::Erofs)
        .expect("import");
    let _active = store
        .acquire_shared_ref(&digest, "vm-active")
        .expect("active ref");

    let err = store
        .remove(&digest)
        .expect_err("active shared ref must block removal");

    assert!(
        matches!(err, StoreError::ImageInUse { ref_count: 1, .. }),
        "got {err:?}"
    );
    assert!(stored_path(root.path(), &digest, ImageKind::Erofs).is_file());
}

#[test]
fn build_minimal_erofs_is_digest_stable_across_rebuilds() {
    let (_root, store) = open_temp_store();
    let source = source_tree();

    let first = store
        .build_minimal_test_image(source.path(), ImageKind::Erofs)
        .expect("first erofs build");
    let second = store
        .build_minimal_test_image(source.path(), ImageKind::Erofs)
        .expect("second erofs build");

    assert_eq!(first, second);
    store.verify(&first).expect("verify erofs build");
}

#[test]
fn build_minimal_ext4_imports_and_verifies() {
    let (_root, store) = open_temp_store();
    let source = source_tree();

    let digest = store
        .build_minimal_test_image(source.path(), ImageKind::Ext4)
        .expect("ext4 build");

    let resolved = store.resolve(&digest).expect("resolve ext4 build");
    assert_eq!(resolved.kind(), ImageKind::Ext4);
    assert!(resolved.size_bytes() > 0);
    store.verify(&digest).expect("verify ext4 build");
}

#[test]
fn shared_ref_acquire_and_release_updates_marker_count_without_deleting_artifact() {
    let (root, store) = open_temp_store();
    let source = erofs_source(root.path(), "shared.erofs", b"shared bytes");
    let digest = store
        .import_existing(&source, ImageKind::Erofs)
        .expect("import shared artifact");
    let artifact = stored_path(root.path(), &digest, ImageKind::Erofs);

    let shared_ref = store
        .acquire_shared_ref(&digest, "vm-shared-a")
        .expect("acquire shared ref");

    assert!(shared_ref.marker_path().is_file());
    assert_eq!(store.shared_ref_count(&digest).unwrap(), 1);

    shared_ref.release().expect("release shared ref");

    assert_eq!(store.shared_ref_count(&digest).unwrap(), 0);
    assert!(
        artifact.is_file(),
        "shared refs track active users; they must not delete canonical artifacts"
    );
}

#[test]
fn shared_ref_duplicate_vm_id_fails_closed() {
    let (root, store) = open_temp_store();
    let source = erofs_source(root.path(), "shared.erofs", b"shared duplicate bytes");
    let digest = store
        .import_existing(&source, ImageKind::Erofs)
        .expect("import shared artifact");
    let _first = store
        .acquire_shared_ref(&digest, "vm-shared-a")
        .expect("first shared ref");

    let err = store
        .acquire_shared_ref(&digest, "vm-shared-a")
        .expect_err("duplicate marker must fail");

    assert!(
        matches!(err, StoreError::SharedRefAlreadyExists { .. }),
        "got {err:?}"
    );
}

#[test]
fn shared_ref_rejects_invalid_vm_marker_name() {
    let (root, store) = open_temp_store();
    let source = erofs_source(root.path(), "shared.erofs", b"shared invalid vm id bytes");
    let digest = store
        .import_existing(&source, ImageKind::Erofs)
        .expect("import shared artifact");

    let err = store
        .acquire_shared_ref(&digest, "../escape")
        .expect_err("invalid marker name must fail");

    assert!(
        matches!(
            err,
            StoreError::InvalidPath {
                reason: "shared ref vm_id must be non-empty ASCII alphanumeric, '.', '_', or '-'",
                ..
            }
        ),
        "got {err:?}"
    );
}

#[test]
fn sweep_shared_refs_removes_only_non_live_markers() {
    let (root, store) = open_temp_store();
    let source = erofs_source(root.path(), "shared.erofs", b"shared sweep bytes");
    let digest = store
        .import_existing(&source, ImageKind::Erofs)
        .expect("import shared artifact");
    let live = store
        .acquire_shared_ref(&digest, "vm-live")
        .expect("live ref");
    let stale = store
        .acquire_shared_ref(&digest, "vm-stale")
        .expect("stale ref");
    let stale_marker = stale.marker_path();
    std::mem::forget(stale);

    let removed = store
        .sweep_shared_refs(["vm-live"])
        .expect("sweep shared refs");

    assert_eq!(removed, 1);
    assert!(live.marker_path().is_file());
    assert!(!stale_marker.exists());
    assert_eq!(store.shared_ref_count(&digest).unwrap(), 1);
    live.release().expect("release live ref");
}
