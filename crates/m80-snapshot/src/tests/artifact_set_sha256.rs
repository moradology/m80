//! Tests for `artifact_set_sha256`.
//! Bead: m80-0tf.1.2

use super::common;
use crate::{artifact_set_sha256, Artifact, ArtifactKind};

fn base_artifacts() -> Vec<Artifact> {
    common::five_artifacts(std::path::Path::new("/snap"))
}

/// Same input always yields the same 32-byte digest.
#[test]
fn artifact_set_sha256_is_deterministic_for_same_input() {
    let arts = base_artifacts();
    let d1 = artifact_set_sha256(&arts);
    let d2 = artifact_set_sha256(&arts);
    assert_eq!(d1, d2, "digest must be deterministic for identical input");
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

/// Empty slice hashes to a fixed SHA-256 of the empty input (all-zero update).
#[test]
fn artifact_set_sha256_empty_slice_is_stable() {
    let d1 = artifact_set_sha256(&[]);
    let d2 = artifact_set_sha256(&[]);
    assert_eq!(d1, d2);
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
