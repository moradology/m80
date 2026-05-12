//! Tests for `artifact_set_sha256`.
//! Bead: m80-0tf.1.2

use super::common;
use crate::{artifact_set_sha256, Artifact, ArtifactKind};

fn base_artifacts() -> Vec<Artifact> {
    common::five_artifacts(std::path::Path::new("/snap"))
}

/// Digest of the five-artifact base set is pinned to a known value.
/// A change in serialization format or hashing logic will break this test
/// visibly rather than silently passing.
#[test]
fn artifact_set_sha256_is_deterministic_for_same_input() {
    let arts = base_artifacts();
    let digest = artifact_set_sha256(&arts);
    // Pin to known expected value (update intentionally if format changes).
    let expected: [u8; 32] = [
        0x81, 0xcc, 0x67, 0x56, 0xaf, 0xd1, 0x62, 0xb4,
        0xda, 0xf9, 0x6c, 0x1b, 0x80, 0x86, 0x54, 0x84,
        0x3e, 0x64, 0xfc, 0x0a, 0x4a, 0x71, 0xaa, 0x57,
        0x03, 0x7d, 0x26, 0xd4, 0x09, 0xe5, 0x25, 0xbf,
    ];
    assert_eq!(digest, expected, "digest of base_artifacts() must match pinned value");
}

/// Flipping one byte in one artifact's sha256 field changes the digest.
#[test]
fn artifact_set_sha256_changes_when_an_artifact_changes() {
    let arts = base_artifacts();
    let original = artifact_set_sha256(&arts);

    let mut mutated = arts;
    // Replace the 'a'-repeat sha256 of the first artifact with 'f'-repeat.
    mutated[0].sha256 = "f".repeat(64);
    let changed = artifact_set_sha256(&mutated);

    assert_ne!(
        original, changed,
        "digest must differ when an artifact's sha256 field changes"
    );
}

/// Same artifacts in a different order produce a different digest.
/// Callers decide canonical order; this function hashes whatever it receives.
#[test]
fn artifact_set_sha256_changes_when_order_changes() {
    let arts = base_artifacts();
    assert!(arts.len() >= 2, "need at least two artifacts");

    let original = artifact_set_sha256(&arts);

    let mut reordered = arts;
    reordered.swap(0, 1);
    let swapped = artifact_set_sha256(&reordered);

    assert_ne!(
        original, swapped,
        "digest must differ when artifact order changes"
    );
}

/// Empty slice produces the SHA-256 of the empty byte sequence. Pinned to the
/// well-known value so a broken hasher initialization is immediately visible.
#[test]
fn artifact_set_sha256_empty_slice_is_stable() {
    let digest = artifact_set_sha256(&[]);
    // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
    let expected: [u8; 32] = [
        0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14,
        0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9, 0x24,
        0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c,
        0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52, 0xb8, 0x55,
    ];
    assert_eq!(digest, expected, "empty-slice digest must match SHA-256 of empty input");
}

/// A single-element slice differs from the five-element slice.
#[test]
fn artifact_set_sha256_single_differs_from_five() {
    let arts = base_artifacts();
    let single = &arts[..1];
    let five = artifact_set_sha256(&arts);
    let one = artifact_set_sha256(single);
    assert_ne!(one, five);
}

/// Changing only the `size` field changes the digest (size is part of the
/// per-artifact bytes that are hashed).
#[test]
fn artifact_set_sha256_changes_when_size_changes() {
    let arts = base_artifacts();
    let original = artifact_set_sha256(&arts);

    let mut mutated = arts;
    mutated[0].size += 1;
    let changed = artifact_set_sha256(&mutated);

    assert_ne!(
        original, changed,
        "digest must differ when an artifact's size field changes"
    );
}

/// Changing the `kind` field changes the digest.
#[test]
fn artifact_set_sha256_changes_when_kind_changes() {
    let arts = base_artifacts();
    let original = artifact_set_sha256(&arts);

    // Swap VmState → Memory on the first artifact.
    let mut mutated = arts;
    mutated[3].kind = ArtifactKind::Memory; // index 3 is VmState in common::five_artifacts
    let changed = artifact_set_sha256(&mutated);

    assert_ne!(
        original, changed,
        "digest must differ when an artifact's kind changes"
    );
}
