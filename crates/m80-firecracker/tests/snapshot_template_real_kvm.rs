//! Real-KVM snapshot-template warm-pool coverage.
//!
//! Each test is ignored by default and expects the standard m80 real-KVM
//! environment variables used by the rest of this crate's integration suite.

mod common;
mod snapshot_template_support;

use std::sync::Arc;
use std::time::Duration;

use m80_firecracker::{FcError, HookSpec, HookSpecSet, HostnameSpec, TemplateStore};
use snapshot_template_support::*;
use tempfile::TempDir;

#[test]
#[ignore = "requires-kvm requires-artifacts requires-snapshot-support"]
fn e2e_1_deterministic_fingerprint() {
    let _guard = real_kvm_test_lock();
    let discovery = real_kvm_discovery();
    let first_temp = TempDir::new().expect("first template store");
    let second_temp = TempDir::new().expect("second template store");
    let suffix = unique_suffix("tplfp");

    let first = build_ready_template_and_read_fingerprint(
        &discovery,
        first_temp.path(),
        HookSpecSet::empty(),
        sandbox_config(format!("{suffix}-a"), None),
        format!("{suffix}a"),
    );
    let second = build_ready_template_and_read_fingerprint(
        &discovery,
        second_temp.path(),
        HookSpecSet::empty(),
        sandbox_config(format!("{suffix}-b"), None),
        format!("{suffix}b"),
    );

    assert_eq!(
        first, second,
        "identical real-KVM template inputs must produce one stable fingerprint"
    );
}

#[test]
#[ignore = "requires-kvm requires-artifacts requires-snapshot-support"]
fn e2e_3_post_restore_reseed() {
    let _guard = real_kvm_test_lock();
    let discovery = real_kvm_discovery();
    let temp = TempDir::new().expect("template store");
    let suffix = unique_suffix("tplrs");
    let pool = template_pool(
        &discovery,
        temp.path(),
        HookSpecSet::new(vec![HookSpec::ReseedSystemdRandomSeed]),
        sandbox_config(format!("{suffix}-base"), None),
        suffix.clone(),
    );
    pool.fill_to_target_blocking()
        .expect("fill reseed template");

    let first = lease_urandom_sample(&pool);
    pool.wait_for_ready(1, Duration::from_secs(120))
        .expect("refill after first reseed lease");
    let second = lease_urandom_sample(&pool);

    assert_eq!(first.len(), 32);
    assert_eq!(second.len(), 32);
    assert_ne!(
        first, second,
        "two restored leases must receive fresh entropy after host nonce reseed"
    );
}

#[test]
#[ignore = "requires-kvm requires-artifacts requires-snapshot-support"]
fn e2e_4_per_lease_machine_id() {
    let _guard = real_kvm_test_lock();
    let discovery = real_kvm_discovery();
    let temp = TempDir::new().expect("template store");
    let suffix = unique_suffix("tplmid");
    let pool = template_pool(
        &discovery,
        temp.path(),
        HookSpecSet::new(vec![HookSpec::RegenMachineId]),
        sandbox_config(format!("{suffix}-base"), None),
        suffix.clone(),
    );
    pool.fill_to_target_blocking()
        .expect("fill machine-id template");

    let first = lease_machine_id(&pool);
    pool.wait_for_ready(1, Duration::from_secs(120))
        .expect("refill after first machine-id lease");
    let second = lease_machine_id(&pool);

    assert_machine_id(&first);
    assert_machine_id(&second);
    assert_ne!(
        first, second,
        "each restored lease must get a fresh machine-id"
    );
}

#[test]
#[ignore = "requires-kvm requires-artifacts requires-snapshot-support"]
fn e2e_5_hostname_hook() {
    let _guard = real_kvm_test_lock();
    let discovery = real_kvm_discovery();
    let temp = TempDir::new().expect("template store");
    let suffix = unique_suffix("tplhost");
    let hostname = format!("m80-{:04x}", common::unique_suffix() % 0x10000);
    let pool = template_pool(
        &discovery,
        temp.path(),
        HookSpecSet::new(vec![HookSpec::SetHostname(
            HostnameSpec::new(&hostname).expect("valid hostname"),
        )]),
        sandbox_config(format!("{suffix}-base"), None),
        suffix,
    );
    pool.fill_to_target_blocking()
        .expect("fill hostname template");

    let mut lease = pool.try_lease().expect("lease hostname slot");
    let kernel_hostname = exec_sh(
        &mut lease,
        "cat /proc/sys/kernel/hostname",
        "read kernel hostname",
    );
    let etc_hostname = exec_sh(&mut lease, "cat /etc/hostname", "read /etc/hostname");
    lease.discard().expect("discard hostname lease");

    assert_eq!(kernel_hostname.trim(), hostname);
    assert_eq!(etc_hostname.trim(), hostname);
}

#[test]
#[ignore = "requires-kvm requires-artifacts requires-snapshot-support"]
fn e2e_6_hook_failure_fail_closed() {
    let _guard = real_kvm_test_lock();
    let discovery = real_kvm_discovery();
    let temp = TempDir::new().expect("template store");
    let suffix = unique_suffix("tplfail");
    let hooks = HookSpecSet::new(vec![HookSpec::RegenMachineId]);
    let sandbox = sandbox_config(format!("{suffix}-base"), None);
    let store = Arc::new(TemplateStore::create(temp.path().join("templates"), 8).expect("store"));
    commit_machine_id_directory_template(&discovery, &store, &sandbox, hooks.clone(), &suffix);
    let pool = template_pool_with_store(&discovery, store, hooks, sandbox, suffix.clone());

    let err = pool
        .fill_to_target_blocking()
        .expect_err("directory machine-id path must make machine-id hook fail closed");
    assert!(
        matches!(
            err,
            FcError::PostRestoreHook(m80_proto::HookError::MachineIdWriteFailed)
        ),
        "expected machine-id hook failure, got {err:?}"
    );
    assert_eq!(pool.snapshot().ready, 0);
    assert_eq!(pool.snapshot().leased, 0);
    assert!(
        matches!(pool.try_lease(), Err(FcError::PoolEmpty { .. })),
        "failed hook restore must not hand back a half-broken lease"
    );
    drop(pool);
    assert_no_run_dirs_with_prefix(&discovery.run_root, &suffix);
}
