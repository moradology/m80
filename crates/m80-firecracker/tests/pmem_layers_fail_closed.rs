//! Pmem layer validation failures stay before kernel-facing side effects.

use std::path::Path;

use m80_image_store::{ImageKind, ImageStore, StoreError};

use m80_firecracker::{
    validate_pmem_layers, ConfigError, ErofsImageRef, FcError, GuestMountPath, ImageDigest,
    PmemLayer, PmemSharing, MAX_PMEM_LAYERS,
};

const VALID_DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const MISSING_DIGEST: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

fn assert_empty_dir(path: &Path) {
    let entries = std::fs::read_dir(path)
        .unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|err| panic!("read entry in {}: {err}", path.display()));
    assert!(
        entries.is_empty(),
        "{} should have no side effects, got {:?}",
        path.display(),
        entries.iter().map(|entry| entry.path()).collect::<Vec<_>>()
    );
}

fn image(digest: &str) -> ErofsImageRef {
    ErofsImageRef::from_digest(ImageDigest::parse(digest).expect("valid digest"))
}

fn mount_path(name: &str) -> GuestMountPath {
    GuestMountPath::parse(&format!("/opt/m80-layers/{name}")).expect("valid mount path")
}

fn layer(name: &str) -> PmemLayer {
    PmemLayer::new(image(VALID_DIGEST), PmemSharing::PerVm, mount_path(name))
}

#[test]
fn bad_digest_fails_before_run_dir_created() {
    let run_root = tempfile::tempdir().expect("run root");

    let err = ImageDigest::parse("abc").expect_err("short digest must fail");

    assert!(matches!(
        err,
        FcError::Config(ConfigError::DigestInvalid {
            reason: "sha256 digest must be 64 lowercase hex characters"
        })
    ));
    assert_empty_dir(run_root.path());
}

#[test]
fn missing_image_in_store_fails_before_jail_materialize() {
    let run_root = tempfile::tempdir().expect("run root");
    let run_dir = run_root.path().join("pmem-missing-image");
    let store_root = tempfile::tempdir().expect("store root");
    let store = ImageStore::open(store_root.path()).expect("store");
    let digest = m80_image_store::ImageDigest::parse(MISSING_DIGEST).expect("valid digest");

    let err = store
        .resolve_as(&digest, ImageKind::Erofs)
        .map_err(FcError::from)
        .expect_err("missing image must fail");

    assert!(
        matches!(err, FcError::ImageStore(StoreError::NotFound { .. })),
        "got {err:?}"
    );
    assert_empty_dir(run_root.path());
    assert!(!run_dir.exists(), "run dir must not be materialized");
}

#[test]
fn escape_mount_path_fails_at_construction() {
    let run_root = tempfile::tempdir().expect("run root");

    let err = GuestMountPath::parse("/opt/m80-layers/../etc/passwd")
        .expect_err("escape mount path must fail");

    assert!(matches!(
        err,
        FcError::Config(ConfigError::MountPathInvalid { .. })
    ));
    assert_empty_dir(run_root.path());
}

#[test]
fn shadowing_proc_fails_at_construction() {
    let run_root = tempfile::tempdir().expect("run root");

    let err = GuestMountPath::parse("/proc").expect_err("proc shadow must fail");

    assert!(matches!(
        err,
        FcError::Config(ConfigError::MountPathShadowsReserved { .. })
    ));
    assert_empty_dir(run_root.path());
}

#[test]
fn shadowing_workspace_fails_at_construction() {
    let run_root = tempfile::tempdir().expect("run root");

    let err = GuestMountPath::parse("/workspace").expect_err("workspace shadow must fail");

    assert!(matches!(
        err,
        FcError::Config(ConfigError::MountPathShadowsReserved { .. })
    ));
    assert_empty_dir(run_root.path());
}

#[test]
fn duplicate_mount_path_fails_at_validate_layers() {
    let run_root = tempfile::tempdir().expect("run root");
    let layers = vec![layer("rust"), layer("rust")];

    let err = validate_pmem_layers(&layers).expect_err("duplicate mount path must fail");

    assert!(matches!(
        err,
        FcError::Config(ConfigError::MountPathDuplicated { .. })
    ));
    assert_empty_dir(run_root.path());
}

#[test]
fn whitespace_in_digest_fails_at_construction() {
    let run_root = tempfile::tempdir().expect("run root");

    let err =
        ImageDigest::parse(" aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            .expect_err("whitespace digest must fail");

    assert!(matches!(
        err,
        FcError::Config(ConfigError::DigestInvalid {
            reason: "sha256 digest must be lowercase hex"
        })
    ));
    assert_empty_dir(run_root.path());
}

#[test]
fn uppercase_in_digest_fails_at_construction() {
    let run_root = tempfile::tempdir().expect("run root");

    let err =
        ImageDigest::parse("Aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            .expect_err("uppercase digest must fail");

    assert!(matches!(
        err,
        FcError::Config(ConfigError::DigestInvalid {
            reason: "sha256 digest must be lowercase hex"
        })
    ));
    assert_empty_dir(run_root.path());
}

#[test]
fn too_many_layers_fails_at_validate_layers() {
    let run_root = tempfile::tempdir().expect("run root");
    let layers = (0..=MAX_PMEM_LAYERS)
        .map(|slot| layer(&format!("layer-{slot}")))
        .collect::<Vec<_>>();

    let err = validate_pmem_layers(&layers).expect_err("too many layers must fail");

    assert!(matches!(
        err,
        FcError::Config(ConfigError::TooManyLayers {
            max: MAX_PMEM_LAYERS,
            got
        }) if got == MAX_PMEM_LAYERS + 1
    ));
    assert_empty_dir(run_root.path());
}
