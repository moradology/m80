//! Scenario 3: Shared construction is typed around an explicit trust witness.
//!
//! `crates/m80-firecracker/src/pmem.rs` carries the compile-fail doctests for
//! invalid forms (`TrustDomainAck::default()` and `PmemSharing::Shared` without
//! an ack). This integration test pins the valid constructor type shape from a
//! downstream crate.

use m80_firecracker::{PmemSharing, TrustDomainAck, TrustReason};

#[test]
fn shared_variant_constructor_requires_trust_domain_ack() {
    let ack_constructor: fn(TrustReason) -> TrustDomainAck = TrustDomainAck::new;
    let shared_constructor: fn(TrustDomainAck) -> PmemSharing = PmemSharing::Shared;

    let sharing = shared_constructor(ack_constructor(TrustReason::SameOperator));
    assert!(matches!(sharing, PmemSharing::Shared(_)));
}
