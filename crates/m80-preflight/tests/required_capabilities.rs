//! Pin the contents of `REQUIRED_CAPABILITIES`.

use caps::Capability;
use m80_preflight::REQUIRED_CAPABILITIES;

#[test]
fn matches_declared_set_in_order() {
    let expected = [
        Capability::CAP_NET_ADMIN,
        Capability::CAP_SYS_ADMIN,
        Capability::CAP_MKNOD,
        Capability::CAP_CHOWN,
        Capability::CAP_FOWNER,
        Capability::CAP_KILL,
    ];
    assert_eq!(REQUIRED_CAPABILITIES, expected, "REQUIRED_CAPABILITIES drifted");
}
