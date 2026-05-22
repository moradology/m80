//! Admission-time validation that the selected `vm_id` would not produce an
//! AF_UNIX socket path longer than the kernel's `sun_path` cap.
//!
//! These tests run without KVM or root: they only exercise the arithmetic
//! check inside `Backend::admit()`. The structural fix complements the
//! shorter `unique_vm_id` helpers in the real-KVM tests by failing closed at
//! the boundary instead of producing an opaque IO error at `bind()` time.

mod common;

use std::path::Path;

use m80_firecracker::{ConfigError, FcError, SandboxConfig};

fn config_with_vm_id(vm_id: Option<&str>) -> SandboxConfig {
    SandboxConfig {
        vm_id: vm_id.map(str::to_owned),
        ..common::sandbox_config()
    }
}

#[test]
fn admit_within_budget_succeeds() {
    // /tmp/m80-test (14) + 2*"persist-fs" (10) + "firecracker" (11) +
    // separators (5) + "root/firecracker.sock" (21) = 71 bytes; fits.
    let backend = common::make_fake_backend(4, Path::new("/tmp/m80-test"));
    backend
        .admit(config_with_vm_id(Some("persist-fs")))
        .expect("short vm_id must admit cleanly");
}

#[test]
fn admit_over_budget_returns_typed_error() {
    let backend = common::make_fake_backend(4, Path::new("/var/lib/m80-run"));
    // /var/lib/m80-run (16) + firecracker basename (11): 28-char vm_id
    // produces 108 bytes, just over the 107-byte usable cap.
    let err = backend
        .admit(config_with_vm_id(Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaa")))
        .expect_err("28-char vm_id under /var/lib/m80-run must be rejected");

    let FcError::Config(ConfigError::VmIdPathBudgetExceeded {
        vm_id,
        path_len,
        budget,
        ..
    }) = err
    else {
        panic!("expected ConfigError::VmIdPathBudgetExceeded, got {err:?}");
    };
    assert_eq!(vm_id.len(), 28);
    assert_eq!(path_len, 108);
    assert_eq!(budget, 107);
}

#[test]
fn admit_just_under_budget_succeeds() {
    let backend = common::make_fake_backend(4, Path::new("/var/lib/m80-run"));
    // V = 27 produces 106 bytes; fits below the 107 cap. (Each extra vm_id
    // byte costs 2 path bytes because the jail layout uses vm_id twice; no V
    // produces exactly 107 with this run_root.)
    backend
        .admit(config_with_vm_id(Some("aaaaaaaaaaaaaaaaaaaaaaaaaaa")))
        .expect("27-char vm_id must admit cleanly");
}

#[test]
fn admit_with_no_vm_id_uses_checked_generated_id() {
    let backend = common::make_fake_backend(4, Path::new("/tmp/m80-test"));
    backend
        .admit(config_with_vm_id(None))
        .expect("None vm_id must admit when generated id fits");
}

#[test]
fn admit_with_no_vm_id_rejects_generated_id_over_budget() {
    let backend = common::make_fake_backend(
        4,
        Path::new("/tmp/m80-run-root-that-is-intentionally-too-long-for-auto-vm-ids"),
    );
    let err = backend
        .admit(config_with_vm_id(None))
        .expect_err("generated vm_id must be checked against the path budget");

    let FcError::Config(ConfigError::VmIdPathBudgetExceeded {
        vm_id,
        path_len,
        budget,
        ..
    }) = err
    else {
        panic!("expected ConfigError::VmIdPathBudgetExceeded, got {err:?}");
    };
    assert!(vm_id.starts_with("vm-"), "unexpected generated id: {vm_id}");
    assert!(path_len > budget);
}

#[test]
fn admit_reserved_run_root_names_fail_before_permit() {
    let backend = common::make_fake_backend(2, Path::new("/var/lib/m80-run"));

    for vm_id in [".preserved", "warm"] {
        let err = backend
            .admit(config_with_vm_id(Some(vm_id)))
            .expect_err("reserved vm_id must be rejected");
        assert!(
            matches!(
                err,
                FcError::InvalidVmId {
                    vm_id: ref observed,
                    ..
                } if observed == vm_id
            ),
            "expected InvalidVmId for {vm_id:?}, got {err:?}"
        );
    }

    let s1 = backend
        .admit(config_with_vm_id(Some("ok-1")))
        .expect("permit 1");
    let s2 = backend
        .admit(config_with_vm_id(Some("ok-2")))
        .expect("permit 2");
    drop((s1, s2));
}

#[test]
fn admit_over_budget_does_not_consume_permit() {
    // The path-budget rejection happens before the semaphore is decremented.
    // After a rejected admit, all permits must still be available.
    let backend = common::make_fake_backend(4, Path::new("/var/lib/m80-run"));
    let _err = backend
        .admit(config_with_vm_id(Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaa")))
        .expect_err("over-budget vm_id must be rejected");

    // Hold all 4 permits; succeeds iff none were consumed by the rejected admit.
    let s1 = backend
        .admit(config_with_vm_id(Some("ok-1")))
        .expect("permit 1");
    let s2 = backend
        .admit(config_with_vm_id(Some("ok-2")))
        .expect("permit 2");
    let s3 = backend
        .admit(config_with_vm_id(Some("ok-3")))
        .expect("permit 3");
    let s4 = backend
        .admit(config_with_vm_id(Some("ok-4")))
        .expect("permit 4");
    drop((s1, s2, s3, s4));
}
