//! Verify that REQUIRED_CAPABILITIES contains exactly the six documented caps
//! in the declared order.

use caps::Capability;
use m80_preflight::REQUIRED_CAPABILITIES;

#[test]
fn contains_exactly_six_caps() {
    assert_eq!(
        REQUIRED_CAPABILITIES.len(),
        6,
        "REQUIRED_CAPABILITIES must contain exactly 6 entries, got {}",
        REQUIRED_CAPABILITIES.len()
    );
}

#[test]
fn contains_cap_net_admin() {
    assert!(
        REQUIRED_CAPABILITIES.contains(&Capability::CAP_NET_ADMIN),
        "CAP_NET_ADMIN must be in REQUIRED_CAPABILITIES"
    );
}

#[test]
fn contains_cap_sys_admin() {
    assert!(
        REQUIRED_CAPABILITIES.contains(&Capability::CAP_SYS_ADMIN),
        "CAP_SYS_ADMIN must be in REQUIRED_CAPABILITIES"
    );
}

#[test]
fn contains_cap_mknod() {
    assert!(
        REQUIRED_CAPABILITIES.contains(&Capability::CAP_MKNOD),
        "CAP_MKNOD must be in REQUIRED_CAPABILITIES"
    );
}

#[test]
fn contains_cap_chown() {
    assert!(
        REQUIRED_CAPABILITIES.contains(&Capability::CAP_CHOWN),
        "CAP_CHOWN must be in REQUIRED_CAPABILITIES"
    );
}

#[test]
fn contains_cap_fowner() {
    assert!(
        REQUIRED_CAPABILITIES.contains(&Capability::CAP_FOWNER),
        "CAP_FOWNER must be in REQUIRED_CAPABILITIES"
    );
}

#[test]
fn contains_cap_kill() {
    assert!(
        REQUIRED_CAPABILITIES.contains(&Capability::CAP_KILL),
        "CAP_KILL must be in REQUIRED_CAPABILITIES"
    );
}
