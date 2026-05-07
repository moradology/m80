use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use m80_firecracker::{
    load_config_from_paths, Backend, BackendConfig, CgroupMode, ConfigFilePaths, ConfigSource,
    FcError,
};
use tempfile::TempDir;

use crate::common;

fn make_backend(max: u32, run_root: &Path) -> Arc<Backend> {
    let config = BackendConfig {
        discovery: common::fake_discovery(run_root),
        max_concurrent_vms: max,
        run_root: run_root.to_path_buf(),
        jail_uid: 3000,
        jail_gid: 3000,
        cgroup_mode: CgroupMode::Disabled,
    };
    Arc::new(Backend::new(config).expect("Backend::new"))
}



#[test]
fn permit_acquired_per_vm() {
    let dir = TempDir::new().unwrap();
    let backend = make_backend(1, dir.path());
    let first = backend.admit(common::sandbox_config_with_id("vm-a")).unwrap();

    let second = backend.admit(common::sandbox_config_with_id("vm-b"));

    assert!(matches!(
        second,
        Err(FcError::AdmissionRefused { limit: 1 })
    ));
    drop(first);
    backend.admit(common::sandbox_config_with_id("vm-c")).unwrap();
}

#[test]
fn reads_max_from_env() {
    let _lock = common::env_lock().lock().unwrap();
    let _restore = common::EnvRestore::capture(common::CONFIG_ENV_KEYS);
    for key in common::CONFIG_ENV_KEYS {
        std::env::remove_var(key);
    }
    let home = TempDir::new().unwrap();
    std::env::set_var("HOME", home.path());
    std::env::set_var("M80_MAX_CONCURRENT_VMS", "13");

    let effective = load_config_from_paths(
        HashMap::new(),
        ConfigFilePaths {
            system: None,
            system_drop_in_dir: None,
            user: None,
            user_drop_in_dir: None,
        },
    )
    .unwrap();
    let field = effective
        .fields
        .iter()
        .find(|field| field.name == "max_concurrent_vms")
        .unwrap();

    assert_eq!(field.value, "13");
    assert_eq!(field.source, ConfigSource::Env);
}

#[test]
fn reports_unavailable_when_pool_full() {
    let dir = TempDir::new().unwrap();
    let backend = make_backend(2, dir.path());
    let first = backend.admit(common::sandbox_config_with_id("vm-a")).unwrap();
    let second = backend.admit(common::sandbox_config_with_id("vm-b")).unwrap();

    let err = backend.admit(common::sandbox_config_with_id("vm-c")).unwrap_err();

    assert_eq!(
        err.to_string(),
        "admission refused: 2 concurrent VMs already running"
    );
    drop(first);
    drop(second);
}
