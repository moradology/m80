use std::collections::HashMap;
use std::ffi::OsString;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use m80_firecracker::{
    load_config_from_paths, Backend, BackendConfig, CgroupMode, ConfigFilePaths, ConfigSource,
    FcError, NetworkPolicy, SandboxConfig,
};
use tempfile::TempDir;

use crate::common;

const ENV_KEYS: &[&str] = &["HOME", "M80_MAX_CONCURRENT_VMS"];

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvRestore {
    values: Vec<(&'static str, Option<OsString>)>,
}

impl EnvRestore {
    fn capture() -> Self {
        Self {
            values: ENV_KEYS
                .iter()
                .map(|key| (*key, std::env::var_os(key)))
                .collect(),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (key, value) in &self.values {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}

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

fn sandbox_config(vm_id: &str) -> SandboxConfig {
    SandboxConfig {
        vm_id: Some(vm_id.to_owned()),
        workspace: None,
        network: NetworkPolicy::NoEgress,
        vcpu_count: None,
        mem_size_mib: None,
        boot_args: None,
        overlay_size_bytes: 512 * 1024 * 1024,
        idle_timeout: None,
        request_id: None,
    }
}

#[test]
fn permit_acquired_per_vm() {
    let dir = TempDir::new().unwrap();
    let backend = make_backend(1, dir.path());
    let first = backend.admit(sandbox_config("vm-a")).unwrap();

    let second = backend.admit(sandbox_config("vm-b"));

    assert!(matches!(
        second,
        Err(FcError::AdmissionRefused { limit: 1 })
    ));
    drop(first);
    backend.admit(sandbox_config("vm-c")).unwrap();
}

#[test]
fn reads_max_from_env() {
    let _lock = env_lock().lock().unwrap();
    let _restore = EnvRestore::capture();
    for key in ENV_KEYS {
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
    let first = backend.admit(sandbox_config("vm-a")).unwrap();
    let second = backend.admit(sandbox_config("vm-b")).unwrap();

    let err = backend.admit(sandbox_config("vm-c")).unwrap_err();

    assert_eq!(
        err.to_string(),
        "admission refused: 2 concurrent VMs already running"
    );
    drop(first);
    drop(second);
}
