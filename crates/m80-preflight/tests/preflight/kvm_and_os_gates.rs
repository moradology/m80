use caps::{Capability, CapsHashSet};
use m80_preflight::{classify_privilege, PreflightError, PrivilegeStatus, REQUIRED_CAPABILITIES};

fn all_required_caps() -> CapsHashSet {
    REQUIRED_CAPABILITIES.iter().copied().collect()
}

#[test]
fn privilege_gate_accepts_effective_root() {
    let status = classify_privilege(0, &CapsHashSet::new()).unwrap();

    assert_eq!(status, PrivilegeStatus::Root);
}

#[test]
fn privilege_gate_accepts_required_effective_capabilities() {
    let status = classify_privilege(1000, &all_required_caps()).unwrap();

    assert_eq!(status, PrivilegeStatus::CapabilityBearing);
}

#[test]
fn privilege_gate_rejects_missing_capabilities() {
    let mut caps = all_required_caps();
    caps.remove(&Capability::CAP_NET_ADMIN);

    let err = classify_privilege(1000, &caps).unwrap_err();

    match err {
        PreflightError::PrivilegeUnavailable { missing_caps } => {
            assert_eq!(missing_caps, vec![Capability::CAP_NET_ADMIN]);
        }
        other => panic!("expected privilege unavailable, got {other:?}"),
    }
}

#[test]
fn privilege_gate_rejects_missing_setpcap_for_jailer_wrapper_pruning() {
    let mut caps = all_required_caps();
    caps.remove(&Capability::CAP_SETPCAP);

    let err = classify_privilege(1000, &caps).unwrap_err();

    match err {
        PreflightError::PrivilegeUnavailable { missing_caps } => {
            assert_eq!(missing_caps, vec![Capability::CAP_SETPCAP]);
        }
        other => panic!("expected privilege unavailable, got {other:?}"),
    }
}
