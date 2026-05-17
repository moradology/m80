//! Concurrent rootfs overlay-template clone regressions.

mod common;

use std::fs::OpenOptions;
use std::io::{Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use m80_storage::{OverlayTemplateCloneMode, Rootfs};
use sha2::{Digest as _, Sha256};

use common::reflink_helpers::assert_sparse_file;

const OVERLAY_SIZE: u64 = 64 * 1024 * 1024;

static TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn concurrent_fill_produces_eight_independent_overlays() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dir = tempfile::tempdir_in(".").unwrap();
    let base = dir.path().join("base.ext4");
    std::fs::write(&base, b"fake-base").unwrap();

    let template = dir.path().join(".rootfs-overlay-template-v1-67108864.ext4");
    let base = Arc::new(base);
    let mut handles = Vec::new();
    for idx in 0..8 {
        let base = Arc::clone(&base);
        let run_dir = dir.path().join(format!("vm-{idx}"));
        std::fs::create_dir(&run_dir).unwrap();
        let overlay = run_dir.join("rootfs.overlay.ext4");
        handles.push(thread::spawn(move || {
            Rootfs::prepare(
                &base,
                &overlay,
                OVERLAY_SIZE,
                OverlayTemplateCloneMode::ByteCopy,
            )
            .unwrap();
            overlay
        }));
    }

    let overlays: Vec<PathBuf> = handles
        .into_iter()
        .map(|handle| handle.join().expect("prepare thread must not panic"))
        .collect();

    assert_eq!(template_count(dir.path()), 1);
    assert!(!dir
        .path()
        .join(".rootfs-overlay-template-v1-67108864.lock")
        .exists());
    assert_sparse_file(&template, OVERLAY_SIZE);
    for overlay in &overlays {
        assert!(overlay.exists(), "{} exists", overlay.display());
        assert_sparse_file(overlay, OVERLAY_SIZE);
    }

    let template_before = sha256(&template);
    for (idx, overlay) in overlays.iter().enumerate() {
        write_marker(overlay, idx);
    }
    assert_eq!(sha256(&template), template_before);
}

fn template_count(run_root: &Path) -> usize {
    std::fs::read_dir(run_root)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with(".rootfs-overlay-template-v1-") && name.ends_with(".ext4")
        })
        .count()
}

fn write_marker(path: &Path, idx: usize) {
    let mut file = OpenOptions::new().write(true).open(path).unwrap();
    file.seek(SeekFrom::Start(1024 * 1024 + idx as u64 * 4096))
        .unwrap();
    file.write_all(format!("m80 overlay marker {idx}\n").as_bytes())
        .unwrap();
}

fn sha256(path: &Path) -> [u8; 32] {
    let bytes = std::fs::read(path).unwrap();
    Sha256::digest(bytes).into()
}
