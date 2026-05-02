//! `Manifest::read` against malformed bytes: the `SchemaVersionProbe` parse
//! fails first and surfaces as `Json`, not `Io`.

use m80_image_manifest::{Manifest, ManifestError};

fn read_bytes(bytes: &[u8]) -> Result<Manifest, ManifestError> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("corrupt.json");
    std::fs::write(&path, bytes).unwrap();
    Manifest::read(&path)
}

#[test]
fn empty_file_surfaces_json_error() {
    let err = read_bytes(b"").unwrap_err();
    assert!(
        matches!(err, ManifestError::Json(_)),
        "empty file must surface as Json error, got {err:?}"
    );
}

#[test]
fn truncated_brace_surfaces_json_error() {
    let err = read_bytes(b"{").unwrap_err();
    assert!(
        matches!(err, ManifestError::Json(_)),
        "truncated `{{` must surface as Json error, got {err:?}"
    );
}

#[test]
fn non_object_root_surfaces_json_error() {
    let err = read_bytes(b"42").unwrap_err();
    assert!(
        matches!(err, ManifestError::Json(_)),
        "non-object root must surface as Json error, got {err:?}"
    );
}

#[test]
fn binary_garbage_surfaces_json_error() {
    let err = read_bytes(&[0x00, 0xFF, 0xDE, 0xAD]).unwrap_err();
    assert!(
        matches!(err, ManifestError::Json(_)),
        "binary garbage must surface as Json error, got {err:?}"
    );
}
