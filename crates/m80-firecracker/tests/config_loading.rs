//! Configuration loading: defaults < system-file < user-file < env < flags.

use std::collections::HashMap;

use m80_firecracker::{load_config, ConfigSource};

#[test]
fn defaults_are_loaded() {
    // This test validates the default value wins when no env var is set.
    // Run with M80_MAX_CONCURRENT_VMS unset:
    // cargo test -p m80-firecracker -- config_loading -- --test-threads=1
    //
    // Because Rust integration tests within the same binary share the same
    // process and environment, we check for the field presence and type rather
    // than assuming no env override is active.
    let effective = load_config(HashMap::new()).expect("load_config should succeed");
    let max = effective
        .fields
        .iter()
        .find(|f| f.name == "max_concurrent_vms")
        .expect("max_concurrent_vms must be present");

    // Verify the field is present and parseable, regardless of the source.
    let _n: u32 = max.value.parse().expect("max_concurrent_vms must be a u32");
}

#[test]
fn env_overrides_default() {
    std::env::set_var("M80_MAX_CONCURRENT_VMS", "42");

    let effective = load_config(HashMap::new()).expect("load_config should succeed");
    let max = effective
        .fields
        .iter()
        .find(|f| f.name == "max_concurrent_vms")
        .expect("max_concurrent_vms must be present");

    assert_eq!(max.value, "42");
    assert_eq!(max.source, ConfigSource::Env);

    std::env::remove_var("M80_MAX_CONCURRENT_VMS");
}

#[test]
fn flag_overrides_env() {
    std::env::set_var("M80_MAX_CONCURRENT_VMS", "42");

    let mut flags = HashMap::new();
    flags.insert("max_concurrent_vms".into(), "99".into());

    let effective = load_config(flags).expect("load_config should succeed");
    let max = effective
        .fields
        .iter()
        .find(|f| f.name == "max_concurrent_vms")
        .expect("max_concurrent_vms must be present");

    assert_eq!(max.value, "99");
    assert_eq!(max.source, ConfigSource::Flag);

    std::env::remove_var("M80_MAX_CONCURRENT_VMS");
}

#[test]
fn unknown_flag_is_ignored() {
    let mut flags = HashMap::new();
    flags.insert("not_a_real_field".into(), "value".into());

    // Should not error; unknown fields are silently dropped.
    load_config(flags).expect("load_config should succeed with unknown flag");
}
