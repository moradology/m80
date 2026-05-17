//! Tests for the internal reflink capability probe.

#[path = "../src/reflink.rs"]
mod reflink;

use std::path::Path;

use reflink::{FsKind, ReflinkCapability, UnsupportedReason};

#[test]
fn tmpfs_probe_reports_known_unsupported_without_ficlone() {
    let shm = Path::new("/dev/shm");
    if !shm.is_dir() {
        eprintln!("SKIP: /dev/shm is unavailable");
        return;
    }

    let result = reflink::probe_for(shm);

    assert_eq!(
        result,
        ReflinkCapability::Unsupported {
            reason: UnsupportedReason::FsTypeKnownNoReflink(FsKind::Tmpfs)
        }
    );
    let ReflinkCapability::Unsupported { reason } = result else {
        unreachable!("assert_eq above pins the variant");
    };
    assert_eq!(reason.fs_kind(), Some(FsKind::Tmpfs));
}

#[test]
fn tempdir_probe_returns_parseable_variant() {
    let dir = tempfile::tempdir().unwrap();

    match reflink::probe_for(dir.path()) {
        ReflinkCapability::Supported
        | ReflinkCapability::Unsupported { .. }
        | ReflinkCapability::ProbeFailed { .. } => {}
    }
}

#[test]
fn cached_probe_returns_same_result_for_same_device() {
    let dir = tempfile::tempdir().unwrap();
    let child = dir.path().join("child");
    std::fs::create_dir(&child).unwrap();

    assert_eq!(
        reflink::device_id_for(dir.path()).unwrap(),
        reflink::device_id_for(&child).unwrap()
    );

    let first = reflink::probe_cached_for(dir.path());
    let second = reflink::probe_cached_for(&child);

    assert_eq!(first, second);
}
