//! Compile-time check that the state-machine types are correctly shaped.
//!
//! These tests don't run real VMs — they verify that:
//! - `Sandbox::new` returns the deferred error in v0.1.
//! - The public type signatures match what m80-cli expects.

use m80_firecracker::{FcError, NetworkPolicy, SandboxConfig};

#[test]
fn sandbox_new_returns_deferred_error_in_v0_1() {
    let config = SandboxConfig {
        vm_id: Some("test-vm".into()),
        workspace: None,
        network: NetworkPolicy::NoEgress,
        vcpu_count: None,
        mem_size_mib: None,
        boot_args: None,
    };

    let err = m80_firecracker::Sandbox::new(config)
        .expect_err("Sandbox::new should return an error in v0.1");

    assert!(
        matches!(err, FcError::Config(_)),
        "expected FcError::Config, got {err:?}"
    );
    let msg = err.to_string();
    assert!(
        msg.contains("Backend::admit"),
        "error message should suggest Backend::admit, got: {msg}"
    );
}
