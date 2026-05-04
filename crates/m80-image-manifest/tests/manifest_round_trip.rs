//! Round-trip + on-disk format invariants: byte-stable, trailing newline,
//! 0644 mode (Unix), JSON keys alphabetical.

mod common;

use m80_image_manifest::Manifest;

#[test]
fn round_trip_byte_equal() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::make_artifacts(dir.path());
    let path = dir.path().join("rootfs.ext4.manifest.json");
    m.write(&path).unwrap();
    let raw1 = std::fs::read(&path).unwrap();
    let m2 = Manifest::read(&path).unwrap();
    let path2 = dir.path().join("rootfs2.ext4.manifest.json");
    m2.write(&path2).unwrap();
    let raw2 = std::fs::read(&path2).unwrap();
    assert_eq!(raw1, raw2, "round-trip must be byte-identical");
}

#[test]
fn trailing_newline_present() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::make_artifacts(dir.path());
    let path = dir.path().join("m.json");
    m.write(&path).unwrap();
    let raw = std::fs::read(&path).unwrap();
    assert_eq!(raw.last(), Some(&b'\n'), "output must end with newline");
}

#[cfg(unix)]
#[test]
fn file_mode_is_0644() {
    use std::os::unix::fs::MetadataExt;
    let dir = tempfile::tempdir().unwrap();
    let m = common::make_artifacts(dir.path());
    let path = dir.path().join("mode_test.json");
    m.write(&path).unwrap();
    let mode = std::fs::metadata(&path).unwrap().mode() & 0o777;
    assert_eq!(mode, 0o644, "manifest file mode must be 0644, got {mode:o}");
}

#[test]
fn keys_are_in_alphabetical_order() {
    let dir = tempfile::tempdir().unwrap();
    let m = common::make_artifacts(dir.path());
    let path = dir.path().join("key_order.json");
    m.write(&path).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    let keys: Vec<&str> = raw
        .lines()
        .filter_map(|l| {
            let trimmed = l.trim();
            if trimmed.starts_with('"') {
                trimmed.split('"').nth(1)
            } else {
                None
            }
        })
        .collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    assert_eq!(keys, sorted, "JSON keys must be in alphabetical order");
}
