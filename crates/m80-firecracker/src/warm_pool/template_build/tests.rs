use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;

use m80_snapshot_template::{
    GuestMountPath, HookSpec, HookSpecSet, ImageDigest, JailBackingPath, PmemTemplateEntry,
    PmemTemplateSharing, TemplateDigest, TemplateFingerprint, TemplateInputs,
    TemplateRestoreLayout, TemplateStore,
};
use tempfile::TempDir;

use super::post_init::{PostInitDigest, PostInitObservables};
use super::*;
use crate::{
    Backend, BackendConfig, CgroupMode, ErofsImageRef, NetworkPolicy, PmemLayer, PmemSharing,
    SandboxConfig, TrustDomainAck, TrustReason, FIRST_LINE_MEM_SIZE_MIB, FIRST_LINE_VCPU_COUNT,
};

mod real_kvm;

#[test]
fn post_init_digest_is_stable_for_same_observables() {
    let fixture = Fixture::fake();
    let config = sandbox_config("digest-stable");
    let inputs = template_inputs_for_current_host(&fixture.backend, &config, HookSpecSet::empty())
        .expect("inputs");

    let left = PostInitDigest::of(&PostInitObservables::from_backend(
        &fixture.backend,
        &config,
        inputs.pmem_image_digest_set(),
    ));
    let right = PostInitDigest::of(&PostInitObservables::from_backend(
        &fixture.backend,
        &config,
        inputs.pmem_image_digest_set(),
    ));

    assert_eq!(left, right);
}

#[test]
fn post_init_digest_changes_when_observable_changes() {
    let fixture = Fixture::fake();
    let config = sandbox_config("digest-base");
    let mut changed = sandbox_config("digest-changed");
    changed.mem_size_mib = Some(FIRST_LINE_MEM_SIZE_MIB + 128);
    let inputs = template_inputs_for_current_host(&fixture.backend, &config, HookSpecSet::empty())
        .expect("inputs");

    let left = PostInitDigest::of(&PostInitObservables::from_backend(
        &fixture.backend,
        &config,
        inputs.pmem_image_digest_set(),
    ));
    let right = PostInitDigest::of(&PostInitObservables::from_backend(
        &fixture.backend,
        &changed,
        inputs.pmem_image_digest_set(),
    ));

    assert_ne!(left, right);
}

#[test]
fn changing_template_input_fields_changes_fingerprint() {
    let fixture = Fixture::fake();
    let mut config = sandbox_config("fingerprint-base");
    config.pmem_layers = vec![pmem_layer("toolchain", "a", PmemSharing::PerVm)];
    let base = template_inputs_for_current_host(&fixture.backend, &config, HookSpecSet::empty())
        .expect("inputs");
    let base_fingerprint = TemplateFingerprint::compute(&base);

    let changed_fields = [
        rebuild_inputs(
            &base,
            "6.18.0",
            base.firecracker_version(),
            base.hook_spec_set(),
        ),
        rebuild_inputs(
            &base,
            base.host_kernel_version(),
            "v1.16.0",
            base.hook_spec_set(),
        ),
        TemplateInputs::new(
            base.host_kernel_version(),
            base.firecracker_version(),
            digest("d"),
            base.pmem_image_digest_set().to_vec(),
            base.post_init_state_digest().clone(),
            base.hook_spec_set().clone(),
        )
        .expect("guest kernel changed"),
        TemplateInputs::new(
            base.host_kernel_version(),
            base.firecracker_version(),
            base.guest_kernel_digest().clone(),
            vec![pmem_entry("toolchain", "b", PmemTemplateSharing::PerVm)],
            base.post_init_state_digest().clone(),
            base.hook_spec_set().clone(),
        )
        .expect("pmem changed"),
        TemplateInputs::new(
            base.host_kernel_version(),
            base.firecracker_version(),
            base.guest_kernel_digest().clone(),
            base.pmem_image_digest_set().to_vec(),
            digest("e"),
            base.hook_spec_set().clone(),
        )
        .expect("post-init changed"),
        rebuild_inputs(
            &base,
            base.host_kernel_version(),
            base.firecracker_version(),
            &HookSpecSet::new(vec![HookSpec::RegenMachineId]),
        ),
    ];

    for changed in changed_fields {
        assert_ne!(base_fingerprint, TemplateFingerprint::compute(&changed));
    }
}

#[test]
fn restore_layout_records_snapshot_paths_and_pmem_backings() {
    let fixture = Fixture::fake();
    let mut config = sandbox_config("layout");
    config.pmem_layers = vec![pmem_layer(
        "toolchain",
        "a",
        PmemSharing::Shared(TrustDomainAck::new(TrustReason::SameOperator)),
    )];
    let inputs = template_inputs_for_current_host(&fixture.backend, &config, HookSpecSet::empty())
        .expect("inputs");

    let layout = restore_layout_for_inputs(&inputs).expect("layout");

    assert_eq!(
        layout.jail_vm_state_path.as_path(),
        Path::new("/snapshot/vm.snap")
    );
    assert_eq!(
        layout.jail_mem_path.as_path(),
        Path::new("/snapshot/mem.snap")
    );
    assert_eq!(layout.pmem_backings, inputs.pmem_image_digest_set());
}

#[test]
fn mismatched_post_init_digest_fails_before_launch() {
    let fixture = Fixture::fake();
    let config = sandbox_config("mismatch");
    let mut inputs =
        template_inputs_for_current_host(&fixture.backend, &config, HookSpecSet::empty())
            .expect("inputs");
    inputs = TemplateInputs::new(
        inputs.host_kernel_version(),
        inputs.firecracker_version(),
        inputs.guest_kernel_digest().clone(),
        inputs.pmem_image_digest_set().to_vec(),
        digest("f"),
        inputs.hook_spec_set().clone(),
    )
    .expect("mismatched inputs");
    let store = TemplateStore::create(fixture.temp.path().join("templates"), 4).expect("store");

    let err =
        build_template(&fixture.backend, inputs, &store, &config).expect_err("mismatch must fail");

    assert!(
        matches!(err, crate::FcError::Config(crate::ConfigError::InvalidValue { field, .. }) if field == "template.inputs")
    );
}

#[test]
fn template_lookup_builds_miss_once_then_hits_cache() {
    let fixture = Fixture::fake();
    let config = sandbox_config("lookup-build");
    let inputs = template_inputs_for_current_host(&fixture.backend, &config, HookSpecSet::empty())
        .expect("inputs");
    let store = TemplateStore::create(fixture.temp.path().join("templates"), 4).expect("store");
    let builds = AtomicUsize::new(0);

    let first = lookup_or_build_template_with(
        &fixture.backend,
        inputs.clone(),
        &store,
        &config,
        |_, inputs, store, _, layout| {
            builds.fetch_add(1, Ordering::SeqCst);
            commit_fake_template(inputs, store, layout)
        },
    )
    .expect("first lookup builds");
    let second = lookup_or_build_template_with(
        &fixture.backend,
        inputs,
        &store,
        &config,
        |_, inputs, store, _, layout| {
            builds.fetch_add(1, Ordering::SeqCst);
            commit_fake_template(inputs, store, layout)
        },
    )
    .expect("second lookup hits cache");

    assert_eq!(builds.load(Ordering::SeqCst), 1);
    assert_eq!(first.reference(), second.reference());
}

#[test]
fn template_capture_cleanup_removes_empty_capture_parent() {
    let temp = TempDir::new().expect("tempdir");
    let capture = TemplateCaptureTarget::new(temp.path(), "abc123");
    std::fs::create_dir_all(&capture.dir).expect("capture dir");
    std::fs::write(capture.dir.join("vm.snap"), b"vm").expect("vm snap");

    capture.cleanup();

    assert!(!capture.dir.exists());
    assert!(!temp.path().join(".template-capture").exists());
}

#[test]
fn template_capture_cleanup_keeps_nonempty_capture_parent() {
    let temp = TempDir::new().expect("tempdir");
    let capture = TemplateCaptureTarget::new(temp.path(), "abc123");
    let sibling = temp.path().join(".template-capture").join("other");
    std::fs::create_dir_all(&capture.dir).expect("capture dir");
    std::fs::create_dir_all(&sibling).expect("sibling capture dir");

    capture.cleanup();

    assert!(!capture.dir.exists());
    assert!(sibling.exists());
    assert!(temp.path().join(".template-capture").exists());
}

struct Fixture {
    temp: TempDir,
    backend: Arc<Backend>,
}

impl Fixture {
    fn fake() -> Self {
        let temp = TempDir::new().expect("tempdir");
        let backend = Arc::new(
            Backend::new(make_backend_config(fake_discovery(temp.path()))).expect("Backend::new"),
        );
        Self { temp, backend }
    }
}

fn rebuild_inputs(
    base: &TemplateInputs,
    host_kernel_version: &str,
    firecracker_version: &str,
    hooks: &HookSpecSet,
) -> TemplateInputs {
    TemplateInputs::new(
        host_kernel_version,
        firecracker_version,
        base.guest_kernel_digest().clone(),
        base.pmem_image_digest_set().to_vec(),
        base.post_init_state_digest().clone(),
        hooks.clone(),
    )
    .expect("inputs")
}

fn sandbox_config(vm_id: &str) -> SandboxConfig {
    SandboxConfig {
        vm_id: Some(vm_id.to_owned()),
        workspace: None,
        network: NetworkPolicy::NoEgress,
        vcpu_count: Some(FIRST_LINE_VCPU_COUNT),
        mem_size_mib: Some(FIRST_LINE_MEM_SIZE_MIB),
        cpuset_cpus: None,
        cpu_template: None,
        fc_log_level: None,
        drive_cache_type: None,
        boot_args: None,
        overlay_size_bytes: 512 * 1024 * 1024,
        overlay_clone_mode: Default::default(),
        idle_timeout: None,
        daemonize: false,
        request_id: None,
        pmem_layers: Vec::new(),
        preallocated_drive_slots: 0,
        one_shot: false,
    }
}

fn make_backend_config(discovery: m80_preflight::Discovery) -> BackendConfig {
    let run_root = discovery.run_root.clone();
    BackendConfig::builder(discovery)
        .max_concurrent_vms(1)
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

fn fake_discovery(run_root: &Path) -> m80_preflight::Discovery {
    let rootfs = tempfile::NamedTempFile::new().expect("fake rootfs");
    let rootfs_path = rootfs.path().to_path_buf();
    let rootfs_file = rootfs.reopen().expect("fake rootfs fd");
    m80_preflight::Discovery {
        firecracker_bin: PathBuf::from("/tmp/firecracker"),
        firecracker_seccomp_filter: PathBuf::from("/tmp/firecracker-seccomp-filter.bin"),
        jailer_bin: PathBuf::from("/tmp/jailer"),
        firecracker_version: "v1.0.0".to_owned(),
        jailer_version: "v1.0.0".to_owned(),
        jailer_harden_bin: PathBuf::from("/tmp/m80-jailer-harden"),
        net_helper_bin: fake_net_helper(run_root),
        kernel: PathBuf::from("/tmp/vmlinux"),
        rootfs: PathBuf::from("/tmp/rootfs.ext4"),
        pinned_rootfs: m80_preflight::PinnedRootfs::from_file(rootfs_path, rootfs_file),
        manifest: m80_image_manifest::Manifest::new(
            "/tmp/m80-guestd".into(),
            "0".repeat(64),
            "v1.0.0".to_owned(),
            52,
            m80_image_manifest::ImageKind::Minimal,
            "/tmp/vmlinux".into(),
            "1".repeat(64),
            m80_image_manifest::KernelKind::Stock,
            None,
            "/tmp/rootfs.ext4".into(),
            "2".repeat(64),
            "M80_READY".to_owned(),
            m80_image_manifest::RootfsFormat::Ext4,
            None,
            None,
        ),
        run_root: run_root.to_path_buf(),
        privilege: m80_preflight::PrivilegeStatus::Root,
        report: Vec::new(),
    }
}

fn fake_net_helper(run_root: &Path) -> PathBuf {
    let _ = run_root;
    static FAKE_NET_HELPER: OnceLock<PathBuf> = OnceLock::new();
    FAKE_NET_HELPER.get_or_init(write_fake_net_helper).clone()
}

fn write_fake_net_helper() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "m80-net-helper-template-build-test-{}",
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
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    path
}

fn pmem_layer(name: &str, digest_byte: &str, sharing: PmemSharing) -> PmemLayer {
    let image =
        ErofsImageRef::from_digest(crate::ImageDigest::parse(&digest_byte.repeat(64)).unwrap());
    let mount_at = crate::GuestMountPath::parse(&format!("/opt/m80-layers/{name}")).unwrap();
    PmemLayer::new(image, sharing, mount_at)
}

fn pmem_entry(name: &str, digest_byte: &str, sharing: PmemTemplateSharing) -> PmemTemplateEntry {
    PmemTemplateEntry::new(
        GuestMountPath::parse(&format!("/opt/m80-layers/{name}")).expect("mount path"),
        ImageDigest::parse(&digest_byte.repeat(64)).expect("digest"),
        sharing,
        JailBackingPath::parse("/pmem.0.img").expect("jail path"),
    )
}

fn digest(byte: &str) -> TemplateDigest {
    TemplateDigest::parse(&byte.repeat(64)).expect("digest")
}

fn commit_fake_template(
    inputs: TemplateInputs,
    store: &TemplateStore,
    restore_layout: TemplateRestoreLayout,
) -> Result<m80_snapshot_template::PinnedTemplate, crate::FcError> {
    let plan = store.reserve(inputs, restore_layout)?;
    std::fs::write(&plan.body_paths().vm_state, b"vm").expect("write vm");
    std::fs::write(&plan.body_paths().mem, b"mem").expect("write mem");
    Ok(store.commit(plan)?)
}
