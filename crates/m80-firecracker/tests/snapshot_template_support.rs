#![allow(dead_code)]

use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use m80_firecracker::{
    Backend, BackendConfig, CgroupMode, HookSpecSet, SandboxConfig, SnapshotPaths, TemplateStore,
    WarmLease, WarmPool, WarmPoolConfig, WarmStrategy, FIRST_LINE_MEM_SIZE_MIB,
    FIRST_LINE_VCPU_COUNT,
};
use m80_proto::ExecRequest;
use m80_snapshot_template::{
    Index, JailBackingPath, TemplateDigest, TemplateFingerprint, TemplateInputs,
    TemplateRestoreLayout,
};
use sha2::{Digest as _, Sha256};

#[path = "common/mod.rs"]
mod common;

pub(crate) fn build_ready_template_and_read_fingerprint(
    discovery: &m80_preflight::Discovery,
    store_parent: &Path,
    hooks: HookSpecSet,
    sandbox: SandboxConfig,
    vm_id_prefix: String,
) -> TemplateFingerprint {
    let store_root = store_parent.join("templates");
    let pool =
        template_pool_with_store_root(discovery, &store_root, hooks, sandbox, vm_id_prefix, 1, 2);
    pool.fill_to_target_blocking()
        .expect("snapshot-template fill");
    let fingerprint = only_index_fingerprint(&store_root);
    drop(pool);
    fingerprint
}

pub(crate) fn template_pool(
    discovery: &m80_preflight::Discovery,
    store_parent: &Path,
    hooks: HookSpecSet,
    sandbox: SandboxConfig,
    vm_id_prefix: String,
) -> WarmPool {
    template_pool_with_target(discovery, store_parent, hooks, sandbox, vm_id_prefix, 1, 2)
}

pub(crate) fn template_pool_with_target(
    discovery: &m80_preflight::Discovery,
    store_parent: &Path,
    hooks: HookSpecSet,
    sandbox: SandboxConfig,
    vm_id_prefix: String,
    target_ready: usize,
    max_concurrent_vms: u32,
) -> WarmPool {
    template_pool_with_store_root(
        discovery,
        &store_parent.join("templates"),
        hooks,
        sandbox,
        vm_id_prefix,
        target_ready,
        max_concurrent_vms,
    )
}

fn template_pool_with_store_root(
    discovery: &m80_preflight::Discovery,
    store_root: &Path,
    hooks: HookSpecSet,
    sandbox: SandboxConfig,
    vm_id_prefix: String,
    target_ready: usize,
    max_concurrent_vms: u32,
) -> WarmPool {
    let store = Arc::new(TemplateStore::create(store_root.to_path_buf(), 8).expect("store"));
    template_pool_with_store_and_target(
        discovery,
        store,
        hooks,
        sandbox,
        vm_id_prefix,
        target_ready,
        max_concurrent_vms,
    )
}

pub(crate) fn template_pool_with_store(
    discovery: &m80_preflight::Discovery,
    store: Arc<TemplateStore>,
    hooks: HookSpecSet,
    sandbox: SandboxConfig,
    vm_id_prefix: String,
) -> WarmPool {
    template_pool_with_store_and_target(discovery, store, hooks, sandbox, vm_id_prefix, 1, 2)
}

pub(crate) fn template_pool_with_store_and_target(
    discovery: &m80_preflight::Discovery,
    store: Arc<TemplateStore>,
    hooks: HookSpecSet,
    sandbox: SandboxConfig,
    vm_id_prefix: String,
    target_ready: usize,
    max_concurrent_vms: u32,
) -> WarmPool {
    let backend = Arc::new(
        Backend::new(make_backend_config(discovery.clone(), max_concurrent_vms))
            .expect("Backend::new template"),
    );
    WarmPool::new(
        backend,
        WarmPoolConfig {
            target_ready,
            sandbox,
            strategy: WarmStrategy::snapshot_restore(store, hooks, true_request()),
            vm_id_prefix,
            cpu_allocator: None,
        },
    )
    .expect("WarmPool::new")
}

pub(crate) fn commit_machine_id_directory_template(
    discovery: &m80_preflight::Discovery,
    store: &TemplateStore,
    sandbox: &SandboxConfig,
    hooks: HookSpecSet,
    suffix: &str,
) {
    let snapshot_dir = discovery
        .run_root
        .join(format!("{suffix}-corrupt-snapshot"));
    let _ = std::fs::remove_dir_all(&snapshot_dir);
    let snapshot = snapshot_paths(&snapshot_dir);
    let backend = Arc::new(
        Backend::new(make_backend_config(discovery.clone(), 1)).expect("Backend::new corrupt"),
    );
    let golden = backend
        .admit(sandbox_config(format!("{suffix}-gold"), None))
        .expect("admit corrupt golden");
    let mut running = golden.launch().expect("launch corrupt golden");
    let _dump = common::RunDirDumpGuard::new(running.run_dir().to_path_buf());
    let _ = running.remove_file("/etc/machine-id");
    running
        .create_dir("/etc/machine-id", Some(0o755), false)
        .expect("create machine-id directory");
    running.capture(snapshot.clone()).expect("capture corrupt");
    running
        .force_kill()
        .expect("kill corrupt golden")
        .delete()
        .expect("delete corrupt golden");

    let inputs = template_inputs_for_test(discovery, sandbox, hooks);
    let layout = TemplateRestoreLayout::new(
        JailBackingPath::parse("/snapshot/vm.snap").expect("vm state jail path"),
        JailBackingPath::parse("/snapshot/mem.snap").expect("mem jail path"),
        Vec::new(),
    );
    let plan = store
        .reserve(inputs, layout)
        .expect("reserve corrupt template");
    std::fs::copy(&snapshot.vm_state, &plan.body_paths().vm_state).expect("copy corrupt vm state");
    std::fs::copy(&snapshot.mem, &plan.body_paths().mem).expect("copy corrupt mem");
    chmod_readable(&plan.body_paths().vm_state);
    chmod_readable(&plan.body_paths().mem);
    store.commit(plan).expect("commit corrupt template");
    std::fs::remove_dir_all(&snapshot_dir).expect("remove corrupt snapshot staging dir");
}

fn chmod_readable(path: &Path) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644))
        .unwrap_or_else(|err| panic!("chmod {}: {err}", path.display()));
}

fn template_inputs_for_test(
    discovery: &m80_preflight::Discovery,
    sandbox: &SandboxConfig,
    hooks: HookSpecSet,
) -> TemplateInputs {
    TemplateInputs::new(
        host_kernel_release(),
        discovery.manifest.expected_firecracker_version.clone(),
        TemplateDigest::parse(&discovery.manifest.kernel_image_sha256).expect("kernel digest"),
        Vec::new(),
        post_init_state_digest_for_test(discovery, sandbox),
        hooks,
    )
    .expect("template inputs")
}

fn post_init_state_digest_for_test(
    discovery: &m80_preflight::Discovery,
    sandbox: &SandboxConfig,
) -> TemplateDigest {
    let manifest = &discovery.manifest;
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, "version", b"PostInitDigestV1");
    hash_u32(&mut hasher, "proto_version", m80_proto::PROTOCOL_VERSION);
    hash_field(
        &mut hasher,
        "image_kind",
        image_kind_name(manifest.image_kind).as_bytes(),
    );
    hash_field(
        &mut hasher,
        "rootfs_format",
        rootfs_format_name(manifest.rootfs_format).as_bytes(),
    );
    hash_field(
        &mut hasher,
        "guestd_sha256",
        manifest.daemon_binary_sha256.as_bytes(),
    );
    hash_field(
        &mut hasher,
        "rootfs_sha256",
        manifest.output_rootfs_sha256.as_bytes(),
    );
    hash_u32(
        &mut hasher,
        "vcpu_count",
        sandbox.vcpu_count.unwrap_or(FIRST_LINE_VCPU_COUNT),
    );
    hash_u32(
        &mut hasher,
        "mem_size_mib",
        sandbox.mem_size_mib.unwrap_or(FIRST_LINE_MEM_SIZE_MIB),
    );
    hash_field(&mut hasher, "cpu_template", b"none");
    hash_field(
        &mut hasher,
        "boot_args",
        sandbox.boot_args.clone().unwrap_or_default().as_bytes(),
    );
    hash_usize(&mut hasher, "pmem_len", 0);
    TemplateDigest::parse(&hex::encode(hasher.finalize())).expect("post-init digest")
}

fn image_kind_name(kind: m80_image_manifest::ImageKind) -> &'static str {
    match kind {
        m80_image_manifest::ImageKind::Ubuntu => "ubuntu",
        m80_image_manifest::ImageKind::Minimal => "minimal",
    }
}

fn rootfs_format_name(format: m80_image_manifest::RootfsFormat) -> &'static str {
    match format {
        m80_image_manifest::RootfsFormat::Ext4 => "ext4",
        m80_image_manifest::RootfsFormat::Erofs => "erofs",
    }
}

fn host_kernel_release() -> String {
    nix::sys::utsname::uname()
        .expect("uname")
        .release()
        .to_string_lossy()
        .into_owned()
}

fn hash_field(hasher: &mut Sha256, label: &str, value: &[u8]) {
    hasher.update(label.as_bytes());
    hasher.update([0]);
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn hash_u32(hasher: &mut Sha256, label: &str, value: u32) {
    hash_field(hasher, label, &value.to_be_bytes());
}

fn hash_usize(hasher: &mut Sha256, label: &str, value: usize) {
    hash_field(hasher, label, &(value as u64).to_be_bytes());
}

pub(crate) fn lease_urandom_sample(pool: &WarmPool) -> Vec<u8> {
    let mut lease = pool.try_lease().expect("lease reseed slot");
    let sample = exec_stdout_bytes(
        &mut lease,
        "/bin/sh",
        &["-c", "dd if=/dev/urandom bs=32 count=1 2>/dev/null"],
        "sample /dev/urandom",
    );
    lease.discard().expect("discard reseed lease");
    sample
}

pub(crate) fn lease_machine_id(pool: &WarmPool) -> String {
    let mut lease = pool.try_lease().expect("lease machine-id slot");
    let machine_id = exec_sh(&mut lease, "cat /etc/machine-id", "read machine-id")
        .trim()
        .to_owned();
    lease.discard().expect("discard machine-id lease");
    machine_id
}

pub(crate) fn exec_sh(lease: &mut WarmLease, script: &str, label: &str) -> String {
    let stdout = exec_stdout_bytes(lease, "/bin/sh", &["-c", script], label);
    String::from_utf8(stdout).unwrap_or_else(|err| panic!("{label}: stdout was not utf8: {err}"))
}

fn exec_stdout_bytes(lease: &mut WarmLease, program: &str, args: &[&str], label: &str) -> Vec<u8> {
    let response = lease
        .exec(ExecRequest {
            program: program.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        })
        .unwrap_or_else(|err| panic!("{label}: {err}"));
    assert_eq!(
        response.exit_code,
        Some(0),
        "{label} failed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&response.stdout),
        String::from_utf8_lossy(&response.stderr)
    );
    response.stdout
}

fn true_request() -> ExecRequest {
    ExecRequest {
        program: "/bin/true".to_owned(),
        args: Vec::new(),
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(5_000),
        streaming: false,
    }
}

fn only_index_fingerprint(store_root: &Path) -> TemplateFingerprint {
    let fingerprints = index_fingerprints(store_root);
    assert_eq!(
        fingerprints.len(),
        1,
        "expected one template entry, got {fingerprints:?}"
    );
    fingerprints[0]
}

pub(crate) fn index_fingerprints(store_root: &Path) -> Vec<TemplateFingerprint> {
    let index = Index::read(&store_root.join("index.json")).expect("read template index");
    index
        .entries
        .into_iter()
        .map(|entry| entry.fingerprint)
        .collect()
}

pub(crate) fn sandbox_config(vm_id: impl Into<String>, boot_args: Option<String>) -> SandboxConfig {
    SandboxConfig {
        vm_id: Some(vm_id.into()),
        vcpu_count: Some(FIRST_LINE_VCPU_COUNT),
        mem_size_mib: Some(FIRST_LINE_MEM_SIZE_MIB),
        boot_args,
        ..common::sandbox_config()
    }
}

fn make_backend_config(
    discovery: m80_preflight::Discovery,
    max_concurrent_vms: u32,
) -> BackendConfig {
    let run_root = discovery.run_root.clone();
    BackendConfig::builder(discovery)
        .max_concurrent_vms(max_concurrent_vms)
        .run_root(run_root)
        .jail_uid(jail_id_from_env("M80_JAIL_UID", 3000))
        .jail_gid(jail_id_from_env("M80_JAIL_GID", 3000))
        .cgroup_mode(CgroupMode::Disabled)
        .build()
}

fn jail_id_from_env(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(default)
}

pub(crate) fn real_kvm_discovery() -> m80_preflight::Discovery {
    let mut discovery = real_kvm_discovery_with_real_net_helper();
    discovery.net_helper_bin = fake_net_helper();
    discovery
}

pub(crate) fn real_kvm_discovery_with_real_net_helper() -> m80_preflight::Discovery {
    if let Ok(run_root) = std::env::var("M80_RUN_ROOT") {
        std::fs::create_dir_all(&run_root).expect("create M80_RUN_ROOT");
    }
    m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts")
}

fn fake_net_helper() -> PathBuf {
    static FAKE_NET_HELPER: OnceLock<PathBuf> = OnceLock::new();
    FAKE_NET_HELPER.get_or_init(write_fake_net_helper).clone()
}

fn write_fake_net_helper() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "m80-net-helper-snapshot-template-test-{}",
        std::process::id()
    ));
    let mut file = std::fs::File::create(&path).expect("fake net helper");
    file.write_all(
        br#"#!/bin/sh
while IFS= read -r _line; do
  printf '%s\n' '{"status":"ok","success":{"kind":"empty"}}'
done
"#,
    )
    .expect("write fake net helper");
    file.sync_all().expect("sync fake net helper");
    drop(file);
    let mut perms = std::fs::metadata(&path)
        .expect("fake net helper metadata")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).expect("chmod fake net helper");
    path
}

pub(crate) fn unique_suffix(prefix: &str) -> String {
    format!("{prefix}-{:04x}", common::unique_suffix() % 0x10000)
}

pub(crate) fn assert_machine_id(value: &str) {
    assert_eq!(value.len(), 32, "machine-id must be 16 bytes in hex");
    assert!(
        value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "machine-id must be lowercase hex, got {value:?}"
    );
}

pub(crate) fn assert_no_run_dirs_with_prefix(run_root: &Path, prefix: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let leaked = std::fs::read_dir(run_root)
            .unwrap_or_else(|err| panic!("read run root {}: {err}", run_root.display()))
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(prefix))
            .collect::<Vec<_>>();
        if leaked.is_empty() {
            return;
        }
        if Instant::now() >= deadline {
            panic!("snapshot-template hook failure leaked run dirs: {leaked:?}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn snapshot_paths(dir: &Path) -> SnapshotPaths {
    std::fs::create_dir_all(dir).expect("create snapshot dir");
    SnapshotPaths {
        vm_state: dir.join("vm.snap"),
        mem: dir.join("mem.snap"),
    }
}

static REAL_KVM_TEST_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn real_kvm_test_lock() -> MutexGuard<'static, ()> {
    REAL_KVM_TEST_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}
