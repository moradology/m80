//! Compile-time check that the state-machine types are correctly shaped.
//!
//! These tests don't run real VMs — they verify that:
//! - `Sandbox::new` returns the deferred error in v0.1.
//! - The public type signatures match what m80-cli expects.

use m80_firecracker::{FcError, NetworkPolicy, RunningSandbox, SandboxConfig, StoppedSandbox};

#[test]
fn sandbox_new_returns_deferred_error_in_v0_1() {
    let config = SandboxConfig {
        vm_id: Some("test-vm".into()),
        workspace: None,
        network: NetworkPolicy::NoEgress,
        vcpu_count: None,
        mem_size_mib: None,
        huge_pages_2m: false,
        cpuset_cpus: None,
        cpu_template: None,
        fc_log_level: None,
        drive_cache_type: None,
        boot_args: None,
        overlay_size_bytes: 512 * 1024 * 1024,
        overlay_clone_mode: Default::default(),
        idle_timeout: None,
        max_lifetime: None,
        daemonize: false,
        request_id: None,
        pmem_layers: Vec::new(),
        preallocated_drive_slots: 0,
        one_shot: false,
    };

    let err = m80_firecracker::Sandbox::new(config)
        .expect_err("Sandbox::new should return an error in v0.1");

    assert!(
        matches!(
            err,
            FcError::UnsupportedOperation {
                operation: "Sandbox::new",
                ..
            }
        ),
        "expected FcError::UnsupportedOperation for Sandbox::new, got {err:?}"
    );
    let msg = err.to_string();
    assert!(
        msg.contains("Backend::admit"),
        "error message should suggest Backend::admit, got: {msg}"
    );
}

#[test]
fn stop_and_force_kill_consume_running_sandbox() {
    let _: fn(RunningSandbox) -> Result<StoppedSandbox, FcError> = RunningSandbox::stop;
    let _: fn(RunningSandbox) -> Result<StoppedSandbox, FcError> = RunningSandbox::force_kill;
}
