//! Pmem layer admission types fail closed before any kernel-facing wiring.

use std::path::Path;

use m80_firecracker::{
    validate_pmem_layers, ConfigError, ErofsImageRef, FcError, GuestMountPath, ImageDigest,
    PmemLayer, PmemSharing, TrustDomainAck, TrustReason, MAX_PMEM_LAYERS,
};

const VALID_DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn digest() -> ImageDigest {
    ImageDigest::parse(VALID_DIGEST).expect("valid digest")
}

fn image() -> ErofsImageRef {
    ErofsImageRef::from_digest(digest())
}

fn mount_path(name: &str) -> GuestMountPath {
    GuestMountPath::parse(&format!("/opt/m80-layers/{name}")).expect("valid mount path")
}

fn layer(name: &str) -> PmemLayer {
    PmemLayer::new(image(), PmemSharing::PerVm, mount_path(name))
}

#[test]
fn valid_digest_parses_and_round_trips() {
    let digest = ImageDigest::parse(VALID_DIGEST).expect("digest parses");

    assert_eq!(digest.as_str(), VALID_DIGEST);
}

#[test]
fn digest_with_wrong_length_is_rejected() {
    let err = ImageDigest::parse("abc").expect_err("short digest must fail");

    assert!(
        matches!(
            err,
            FcError::Config(ConfigError::DigestInvalid {
                reason: "sha256 digest must be 64 lowercase hex characters"
            })
        ),
        "got {err:?}"
    );
}

#[test]
fn digest_with_uppercase_hex_is_rejected() {
    let err =
        ImageDigest::parse("Aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            .expect_err("uppercase digest must fail");

    assert!(
        matches!(
            err,
            FcError::Config(ConfigError::DigestInvalid {
                reason: "sha256 digest must be lowercase hex"
            })
        ),
        "got {err:?}"
    );
}

#[test]
fn digest_with_non_hex_character_is_rejected() {
    let err =
        ImageDigest::parse("gaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            .expect_err("non-hex digest must fail");

    assert!(
        matches!(
            err,
            FcError::Config(ConfigError::DigestInvalid {
                reason: "sha256 digest must be lowercase hex"
            })
        ),
        "got {err:?}"
    );
}

#[test]
fn digest_with_whitespace_is_rejected() {
    let err =
        ImageDigest::parse(" aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            .expect_err("whitespace digest must fail");

    assert!(
        matches!(err, FcError::Config(ConfigError::DigestInvalid { .. })),
        "got {err:?}"
    );
}

#[test]
fn valid_mount_path_round_trips() {
    let path =
        GuestMountPath::parse("/opt/m80-layers/rust-1.82_x86.64").expect("mount path parses");

    assert_eq!(
        path.as_path(),
        Path::new("/opt/m80-layers/rust-1.82_x86.64")
    );
}

#[test]
fn relative_mount_path_is_rejected() {
    let err = GuestMountPath::parse("opt/m80-layers/rust").expect_err("relative path must fail");

    assert!(
        matches!(
            err,
            FcError::Config(ConfigError::MountPathInvalid {
                reason: "path must be absolute"
            })
        ),
        "got {err:?}"
    );
}

#[test]
fn dot_mount_component_is_rejected() {
    let err = GuestMountPath::parse("/opt/m80-layers/.").expect_err("dot path must fail");

    assert!(
        matches!(
            err,
            FcError::Config(ConfigError::MountPathInvalid {
                reason: "path must not contain . or .. components"
            })
        ),
        "got {err:?}"
    );
}

#[test]
fn embedded_dot_mount_component_is_rejected() {
    let err = GuestMountPath::parse("/opt/m80-layers/./rust").expect_err("dot component must fail");

    assert!(
        matches!(
            err,
            FcError::Config(ConfigError::MountPathInvalid {
                reason: "path must match /opt/m80-layers/<name>"
            })
        ),
        "got {err:?}"
    );
}

#[test]
fn parent_mount_component_is_rejected() {
    let err = GuestMountPath::parse("/opt/m80-layers/..").expect_err("parent path must fail");

    assert!(
        matches!(
            err,
            FcError::Config(ConfigError::MountPathInvalid {
                reason: "path must not contain . or .. components"
            })
        ),
        "got {err:?}"
    );
}

#[test]
fn reserved_root_mount_path_is_rejected() {
    let err = GuestMountPath::parse("/").expect_err("reserved root must fail");

    assert!(
        matches!(
            err,
            FcError::Config(ConfigError::MountPathShadowsReserved { .. })
        ),
        "got {err:?}"
    );
}

#[test]
fn reserved_proc_mount_path_is_rejected() {
    let err = GuestMountPath::parse("/proc").expect_err("reserved proc mount must fail");

    assert!(
        matches!(
            err,
            FcError::Config(ConfigError::MountPathShadowsReserved { .. })
        ),
        "got {err:?}"
    );
}

#[test]
fn reserved_workspace_subtree_is_rejected() {
    let err = GuestMountPath::parse("/workspace/toolchain")
        .expect_err("reserved workspace subtree must fail");

    assert!(
        matches!(
            err,
            FcError::Config(ConfigError::MountPathShadowsReserved { .. })
        ),
        "got {err:?}"
    );
}

#[test]
fn mount_path_without_layer_prefix_is_rejected() {
    let err =
        GuestMountPath::parse("/opt/not-m80-layers/rust").expect_err("wrong prefix must fail");

    assert!(
        matches!(
            err,
            FcError::Config(ConfigError::MountPathInvalid {
                reason: "path must match /opt/m80-layers/<name>"
            })
        ),
        "got {err:?}"
    );
}

#[test]
fn empty_layer_name_is_rejected() {
    let err = GuestMountPath::parse("/opt/m80-layers/").expect_err("empty name must fail");

    assert!(
        matches!(
            err,
            FcError::Config(ConfigError::MountPathInvalid {
                reason: "layer name must be 1..=64 characters"
            })
        ),
        "got {err:?}"
    );
}

#[test]
fn layer_name_with_space_is_rejected() {
    let err = GuestMountPath::parse("/opt/m80-layers/rust 1.82").expect_err("space must fail");

    assert!(
        matches!(
            err,
            FcError::Config(ConfigError::MountPathInvalid {
                reason: "layer name must contain only [A-Za-z0-9._-]"
            })
        ),
        "got {err:?}"
    );
}

#[test]
fn layer_name_longer_than_64_is_rejected() {
    let long_name = "a".repeat(65);
    let err = GuestMountPath::parse(&format!("/opt/m80-layers/{long_name}"))
        .expect_err("long name must fail");

    assert!(
        matches!(
            err,
            FcError::Config(ConfigError::MountPathInvalid {
                reason: "layer name must be 1..=64 characters"
            })
        ),
        "got {err:?}"
    );
}

#[test]
fn deeper_mount_path_is_rejected() {
    let err = GuestMountPath::parse("/opt/m80-layers/rust/bin")
        .expect_err("path below layer name must fail");

    assert!(
        matches!(
            err,
            FcError::Config(ConfigError::MountPathInvalid {
                reason: "path must match /opt/m80-layers/<name>"
            })
        ),
        "got {err:?}"
    );
}

#[test]
fn erofs_image_ref_exposes_digest() {
    let digest = digest();
    let image = ErofsImageRef::from_digest(digest.clone());

    assert_eq!(image.digest(), &digest);
}

#[test]
fn pmem_layer_exposes_validated_parts() {
    let image = image();
    let mount_at = mount_path("rust");
    let layer = PmemLayer::new(image.clone(), PmemSharing::PerVm, mount_at.clone());

    assert_eq!(layer.image(), &image);
    assert_eq!(layer.sharing(), PmemSharing::PerVm);
    assert_eq!(layer.mount_at(), &mount_at);
}

#[test]
fn duplicate_mount_path_is_rejected() {
    let layers = [layer("rust"), layer("rust")];

    let err = validate_pmem_layers(&layers).expect_err("duplicate mount path must fail");

    assert!(
        matches!(
            err,
            FcError::Config(ConfigError::MountPathDuplicated { .. })
        ),
        "got {err:?}"
    );
}

#[test]
fn too_many_layers_is_rejected() {
    let layers = (0..=MAX_PMEM_LAYERS)
        .map(|idx| layer(&format!("layer-{idx}")))
        .collect::<Vec<_>>();

    let err = validate_pmem_layers(&layers).expect_err("too many layers must fail");

    assert!(
        matches!(
            err,
            FcError::Config(ConfigError::TooManyLayers {
                max: MAX_PMEM_LAYERS,
                got
            }) if got == MAX_PMEM_LAYERS + 1
        ),
        "got {err:?}"
    );
}

#[test]
fn max_layer_count_with_unique_mounts_is_accepted() {
    let layers = (0..MAX_PMEM_LAYERS)
        .map(|idx| layer(&format!("layer-{idx}")))
        .collect::<Vec<_>>();

    validate_pmem_layers(&layers).expect("max layer count with unique paths is accepted");
}

#[test]
fn trust_domain_ack_exposes_finite_reason() {
    let ack = TrustDomainAck::new(TrustReason::SameOperator);

    assert_eq!(ack.reason(), TrustReason::SameOperator);
}

#[test]
fn trust_reason_variant_set_is_pinned() {
    let observed = TrustReason::variants()
        .iter()
        .copied()
        .map(|reason| match reason {
            TrustReason::SameOperator => "same_operator",
            TrustReason::KubernetesSameNamespace => "kubernetes_same_namespace",
            TrustReason::ResearchSandbox => "research_sandbox",
        })
        .collect::<Vec<_>>();

    assert_eq!(
        observed,
        vec![
            "same_operator",
            "kubernetes_same_namespace",
            "research_sandbox"
        ]
    );
}

#[test]
fn shared_pmem_is_admitted_with_typed_trust_ack() {
    let shared = PmemSharing::Shared(TrustDomainAck::new(TrustReason::SameOperator));
    let layers = [PmemLayer::new(image(), shared, mount_path("rust"))];

    validate_pmem_layers(&layers).expect("shared pmem with trust ack should validate");
}

#[test]
fn pmem_sharing_is_exhaustively_matchable() {
    let cases = [
        PmemSharing::PerVm,
        PmemSharing::Shared(TrustDomainAck::new(TrustReason::SameOperator)),
    ];
    let labels = cases
        .into_iter()
        .map(|sharing| match sharing {
            PmemSharing::PerVm => "per-vm",
            PmemSharing::Shared(ack) => match ack.reason() {
                TrustReason::SameOperator => "shared:same-operator",
                TrustReason::KubernetesSameNamespace => "shared:kubernetes-same-namespace",
                TrustReason::ResearchSandbox => "shared:research-sandbox",
            },
        })
        .collect::<Vec<_>>();

    assert_eq!(labels, vec!["per-vm", "shared:same-operator"]);
}
