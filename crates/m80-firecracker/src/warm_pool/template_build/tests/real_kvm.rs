use super::*;
use crate::{WarmPool, WarmPoolConfig, WarmStrategy};
use std::sync::{Arc, Mutex, MutexGuard};

#[test]
#[ignore = "requires-kvm requires-snapshot-support"]
fn injected_after_capture_failure_leaves_store_miss() {
    let _guard = real_kvm_test_lock();
    let discovery = real_kvm_discovery();
    let backend = Arc::new(Backend::new(make_backend_config(discovery)).expect("Backend::new"));
    let config = real_kvm_sandbox_config("tpl-f");
    let inputs =
        template_inputs_for_current_host(&backend, &config, HookSpecSet::empty()).expect("inputs");
    let temp = TempDir::new().expect("temp");
    let store = TemplateStore::create(temp.path().join("templates"), 4).expect("store");
    let fingerprint = TemplateFingerprint::compute(&inputs);

    fail_next_template_build_after_capture_for_test();
    let err = build_template(&backend, inputs.clone(), &store, &config)
        .expect_err("injected post-capture failure must fail");

    assert!(
        matches!(err, crate::FcError::Config(crate::ConfigError::InvalidValue { field, .. }) if field == "template.build")
    );
    assert!(!store.template_dir(&fingerprint).exists());
    assert!(store.lookup(&inputs).expect("lookup").is_none());
}

#[test]
#[ignore = "requires-kvm requires-snapshot-support"]
fn identical_real_kvm_template_builds_produce_matching_fingerprints() {
    let _guard = real_kvm_test_lock();
    let discovery = real_kvm_discovery();
    let backend = Arc::new(Backend::new(make_backend_config(discovery)).expect("Backend::new"));
    let config = real_kvm_sandbox_config("tpl-g");
    let inputs =
        template_inputs_for_current_host(&backend, &config, HookSpecSet::empty()).expect("inputs");
    let first_temp = TempDir::new().expect("first temp");
    let second_temp = TempDir::new().expect("second temp");
    let first_store =
        TemplateStore::create(first_temp.path().join("templates"), 4).expect("first store");
    let second_store =
        TemplateStore::create(second_temp.path().join("templates"), 4).expect("second store");

    let first =
        build_template(&backend, inputs.clone(), &first_store, &config).expect("first template");
    let second = build_template(&backend, inputs, &second_store, &config).expect("second template");

    assert_eq!(
        first.reference().fingerprint(),
        second.reference().fingerprint()
    );
    assert_eq!(first.manifest().inputs, second.manifest().inputs);
    assert_eq!(
        first.manifest().restore_layout,
        second.manifest().restore_layout
    );
}

#[test]
#[ignore = "requires-kvm requires-snapshot-support"]
fn store_resident_template_restores_from_outside_run_root() {
    let _guard = real_kvm_test_lock();
    let discovery = real_kvm_discovery();
    let backend =
        Arc::new(Backend::new(make_backend_config(discovery.clone())).expect("Backend::new"));
    let config = real_kvm_sandbox_config("tpl-b");
    let inputs =
        template_inputs_for_current_host(&backend, &config, HookSpecSet::empty()).expect("inputs");
    let temp = TempDir::new().expect("outside store temp");
    let store = TemplateStore::create(temp.path().join("templates"), 4).expect("store");
    let pinned = build_template(&backend, inputs, &store, &config).expect("template");

    let restore = backend
        .admit(real_kvm_sandbox_config("tpl-r"))
        .expect("admit restore");
    let mut restored = restore
        .launch_from_template_body_with_hooks(&pinned, &discovery, crate::HookSpecSet::empty())
        .expect("restore template");
    let response = restored
        .exec(m80_proto::ExecRequest {
            program: "/bin/echo".into(),
            args: vec!["template-restored".into()],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        })
        .expect("exec restored");

    assert_eq!(response.exit_code, Some(0));
    assert_eq!(
        String::from_utf8_lossy(&response.stdout).trim(),
        "template-restored"
    );
    restored.stop().expect("stop").delete().expect("delete");
}

#[test]
#[ignore = "requires-kvm requires-snapshot-support"]
fn snapshot_restore_warm_strategy_fills_ready_slot() {
    let _guard = real_kvm_test_lock();
    let discovery = real_kvm_discovery();
    let backend =
        Arc::new(Backend::new(make_backend_config(discovery.clone())).expect("Backend::new"));
    let temp = TempDir::new().expect("outside store temp");
    let store = Arc::new(TemplateStore::create(temp.path().join("templates"), 4).expect("store"));
    let pool = WarmPool::new(
        Arc::clone(&backend),
        WarmPoolConfig {
            target_ready: 1,
            sandbox: real_kvm_sandbox_config("tpls"),
            strategy: WarmStrategy::snapshot_restore(store, HookSpecSet::empty(), true_request()),
            vm_id_prefix: "tp".to_owned(),
            cpu_allocator: None,
        },
    )
    .expect("warm pool");

    pool.fill_to_target_blocking()
        .expect("snapshot-template fill");
    assert_eq!(pool.snapshot().ready, 1);
}

fn real_kvm_sandbox_config(vm_id: &str) -> SandboxConfig {
    SandboxConfig {
        vcpu_count: Some(FIRST_LINE_VCPU_COUNT),
        mem_size_mib: Some(FIRST_LINE_MEM_SIZE_MIB),
        huge_pages_2m: false,
        ..sandbox_config(vm_id)
    }
}

fn true_request() -> m80_proto::ExecRequest {
    m80_proto::ExecRequest {
        program: "/bin/true".to_owned(),
        args: Vec::new(),
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(5_000),
        streaming: false,
    }
}

fn real_kvm_discovery() -> m80_preflight::Discovery {
    if let Ok(run_root) = std::env::var("M80_RUN_ROOT") {
        std::fs::create_dir_all(&run_root).expect("create M80_RUN_ROOT");
    }
    let mut discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    discovery.net_helper_bin = fake_net_helper(&discovery.run_root);
    discovery
}

static REAL_KVM_TEST_LOCK: Mutex<()> = Mutex::new(());

fn real_kvm_test_lock() -> MutexGuard<'static, ()> {
    REAL_KVM_TEST_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}
