//! Scenario 6: Shared pmem exposes no caller-provided writability hint.

use m80_firecracker::{
    ErofsImageRef, GuestMountPath, ImageDigest, PmemLayer, PmemSharing, TrustDomainAck, TrustReason,
};

const VALID_DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn shared_pmem_api_shape_has_no_writability_parameter() {
    let shared_constructor: fn(TrustDomainAck) -> PmemSharing = PmemSharing::Shared;
    let layer_constructor: fn(ErofsImageRef, PmemSharing, GuestMountPath) -> PmemLayer =
        PmemLayer::new;

    let digest = ImageDigest::parse(VALID_DIGEST).expect("valid digest");
    let image = ErofsImageRef::from_digest(digest);
    let sharing = shared_constructor(TrustDomainAck::new(TrustReason::SameOperator));
    let mount = GuestMountPath::parse("/opt/m80-layers/shared").expect("valid mount path");
    let layer = layer_constructor(image, sharing, mount);

    assert!(matches!(layer.sharing(), PmemSharing::Shared(_)));
}
