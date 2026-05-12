//! Shared test helpers for `m80-image-manifest` integration tests.
//!
//! Each `tests/*.rs` file is its own integration crate, so we share via
//! `mod common;` rather than via a `pub use`.

use std::path::Path;

use m80_image_manifest::{ImageKind, KernelKind, Manifest};
use sha2::{Digest, Sha256};

/// Write four dummy artifact files into `dir` and return a `Manifest` whose
/// path + sha256 fields point at them. The image_kind is `Ubuntu` so source
/// rootfs Options are populated.
pub(crate) fn make_artifacts(dir: &Path) -> Manifest {
    for name in &["vmlinux", "source.ext4", "output.ext4", "guestd"] {
        std::fs::write(dir.join(name), name.as_bytes()).unwrap();
    }
    Manifest::new(
        dir.join("guestd"),
        hex::encode(Sha256::digest(b"guestd")),
        "v1.15.1".into(),
        8080,
        ImageKind::Ubuntu,
        dir.join("vmlinux"),
        hex::encode(Sha256::digest(b"vmlinux")),
        KernelKind::Stock,
        None,
        dir.join("output.ext4"),
        hex::encode(Sha256::digest(b"output.ext4")),
        "READY".into(),
        Some(dir.join("source.ext4")),
        Some(hex::encode(Sha256::digest(b"source.ext4"))),
    )
}

/// Same as [`make_artifacts`] but for `ImageKind::Minimal` — only the
/// kernel, output rootfs, and daemon binary exist; all source-rootfs Options
/// are `None`.
///
/// Each `tests/*.rs` is its own crate via `mod common;`, so functions
/// not used by a particular test file are flagged dead from that
/// crate's perspective. Suppress at the function level.
#[allow(dead_code)]
pub(crate) fn make_minimal_artifacts(dir: &Path) -> Manifest {
    for name in &["vmlinux", "output.ext4", "guestd"] {
        std::fs::write(dir.join(name), name.as_bytes()).unwrap();
    }
    Manifest::new(
        dir.join("guestd"),
        hex::encode(Sha256::digest(b"guestd")),
        "v1.15.1".into(),
        8080,
        ImageKind::Minimal,
        dir.join("vmlinux"),
        hex::encode(Sha256::digest(b"vmlinux")),
        KernelKind::Stock,
        None,
        dir.join("output.ext4"),
        hex::encode(Sha256::digest(b"output.ext4")),
        "READY".into(),
        None,
        None,
    )
}
