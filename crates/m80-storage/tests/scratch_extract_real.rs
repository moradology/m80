//! Integration tests for `Scratch::extract` that require root + loop device.
//!
//! All tests are `#[ignore]`; run with:
//!   sudo cargo test -p m80-storage --test scratch_extract_real -- --ignored

use m80_storage::Scratch;

/// Returns `true` iff the test is running as root. Tests should early-return
/// when this returns `false` so non-root invocations don't fail mid-operation.
fn require_root() -> bool {
    if nix::unistd::Uid::effective().is_root() {
        true
    } else {
        eprintln!("[scratch_extract_real] SKIP: not running as root");
        false
    }
}

/// Full create → extract round trip.
///
/// Creates a workspace, formats + hydrates a scratch image, then extracts it
/// back and verifies the ChangeSet matches.
#[test]
#[ignore = "requires root and a loop device"]
fn scratch_extract_round_trips_workspace() {
    if !require_root() { return; }

    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(workspace.join("a.txt"), b"alpha").unwrap();
    std::fs::write(workspace.join("b.txt"), b"beta").unwrap();

    let image = dir.path().join("scratch.ext4");
    Scratch::create(&workspace, &image, 64 * 1024 * 1024).expect("create");

    let into = dir.path().join("extracted");
    let cs = Scratch::extract(&image, &into).expect("extract");

    assert!(into.exists(), "into must exist after extract");
    assert!(cs.rejected.is_empty(), "no rejections expected");
    // staged contains files + the root directory itself is not listed.
    let staged_names: Vec<_> = cs
        .staged
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    assert!(staged_names.contains(&"a.txt".to_owned()), "a.txt must be staged: {staged_names:?}");
    assert!(staged_names.contains(&"b.txt".to_owned()), "b.txt must be staged: {staged_names:?}");
    assert!(cs.total_bytes > 0, "total_bytes must be non-zero");
}

#[test]
#[ignore = "requires root and a loop device"]
fn scratch_extract_rejects_into_already_exists() {
    if !require_root() { return; }

    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();

    let image = dir.path().join("scratch.ext4");
    Scratch::create(&workspace, &image, 64 * 1024 * 1024).expect("create");

    // Create into before extract — must fail with SwapFailed.
    let into = dir.path().join("already_exists");
    std::fs::create_dir_all(&into).unwrap();

    let err = Scratch::extract(&image, &into).unwrap_err();
    assert!(
        matches!(err, m80_storage::StorageError::SwapFailed),
        "expected SwapFailed, got {err:?}"
    );
}
