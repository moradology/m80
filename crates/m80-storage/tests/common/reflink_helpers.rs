//! Shared assertions for rootfs overlay-template clone tests.

use std::os::unix::fs::MetadataExt as _;
use std::path::Path;
use std::process::Command;

#[allow(dead_code)]
pub(crate) fn assert_sparse_file(path: &Path, apparent_size: u64) {
    let meta = std::fs::metadata(path).unwrap();
    assert_eq!(
        meta.len(),
        apparent_size,
        "{} apparent size",
        path.display()
    );
    let allocated = meta.blocks() * 512;
    assert!(
        allocated < apparent_size / 2,
        "{} must be sparse: allocated {allocated} bytes for apparent size {apparent_size}",
        path.display()
    );
}

#[allow(dead_code)]
pub(crate) fn assert_reflink_always_is_unsupported(dir: &Path) {
    let source = dir.join("reflink-source");
    let dest = dir.join("reflink-dest");
    std::fs::write(&source, b"x").unwrap();
    let output = Command::new("cp")
        .arg("--reflink=always")
        .arg(&source)
        .arg(&dest)
        .output()
        .expect("cp must run");
    assert!(
        !output.status.success(),
        "test directory {} unexpectedly supports mandatory reflink",
        dir.display()
    );
}

#[allow(dead_code)]
pub(crate) fn assert_debugfs_can_read_ext4(path: &Path) {
    let output = Command::new("debugfs")
        .arg("-R")
        .arg("stats")
        .arg(path)
        .output()
        .expect("debugfs must run");
    assert!(
        output.status.success(),
        "debugfs stats {} failed: status={:?}\nstdout={}\nstderr={}",
        path.display(),
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
