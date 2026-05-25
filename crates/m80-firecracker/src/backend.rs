//! [`Backend`] implementation: construction, admission, effective-config query,
//! and stale run-root recovery.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tracing::warn;

use m80_image_store::{ImageStore, DEFAULT_STORE_ROOT};
use m80_jailer::inspect_run_dir;
use m80_snapshot_template::{HookSpecSet, PinnedTemplate, TemplateInputs, TemplateStore};

#[cfg(not(test))]
use crate::capabilities::drop_parent_cap_net_admin;
use crate::error::{ConfigError, FcError};
use crate::layout::{socket_path_len, SUN_PATH_BUDGET};
use crate::network_helper::backend_network_helper;
use crate::preboot::validate_caller_boot_args_if_present;
use crate::runroot::{run_dir_liveness, RunDirLiveness};
use crate::types::{
    AdmissionPermit, Backend, BackendConfig, CgroupMode, ConfigSource, EffectiveConfig,
    EffectiveField, Sandbox, SandboxConfig, FIRST_LINE_MEM_SIZE_MIB,
};

impl Backend {
    /// Construct a backend handle.
    ///
    /// `config.discovery` must already be populated by the caller from
    /// `m80-preflight::run()`. No preflight is re-run here.
    pub fn new(config: BackendConfig) -> Result<Self, FcError> {
        let effective = build_effective_config(&config);
        Self::new_with_effective_config(config, effective)
    }

    /// Construct a backend handle while preserving the caller's annotated
    /// effective configuration snapshot.
    ///
    /// Use this when the caller built `config` from `load_config` and wants
    /// `show_effective_config()` to retain source labels instead of rebuilding
    /// a default-tagged snapshot from typed backend fields.
    pub fn new_with_effective_config(
        config: BackendConfig,
        effective: EffectiveConfig,
    ) -> Result<Self, FcError> {
        Self::new_inner(config, effective, ParentCapabilityDrop::Run)
    }

    /// Construct a backend for hostless integration tests that exercise
    /// admission/recovery logic without invoking the one-way parent capability
    /// hardening path.
    ///
    /// Real-KVM and security tests must use [`Backend::new`] so they keep
    /// proving the production capability behavior.
    #[doc(hidden)]
    pub fn new_without_parent_capability_drop_for_tests(
        config: BackendConfig,
    ) -> Result<Self, FcError> {
        let effective = build_effective_config(&config);
        Self::new_inner(config, effective, ParentCapabilityDrop::SkipForHostlessTest)
    }

    fn new_inner(
        config: BackendConfig,
        effective: EffectiveConfig,
        parent_capability_drop: ParentCapabilityDrop,
    ) -> Result<Self, FcError> {
        let permits = config.max_concurrent_vms;
        let semaphore = Arc::new(Mutex::new(permits));
        let network_helper = backend_network_helper(&config.discovery.net_helper_bin)?;
        #[cfg(test)]
        let _ = parent_capability_drop;
        #[cfg(not(test))]
        if parent_capability_drop == ParentCapabilityDrop::Run {
            drop_parent_cap_net_admin()?;
        }
        let backend = Backend {
            config,
            effective,
            semaphore,
            network_helper,
        };
        if let Err(e) = backend.recover_stale_run_root(false) {
            warn!(err = %e, "Backend::new: stale run-root recovery failed");
        }
        if let Err(e) = backend.sweep_stale_shared_pmem_refs() {
            warn!(err = %e, "Backend::new: shared pmem ref sweep failed");
        }
        Ok(backend)
    }

    /// Acquire one admission permit and return a [`Sandbox`] in Created state.
    ///
    /// Fails fast with [`FcError::AdmissionRefused`] when no permits are
    /// available, or with [`ConfigError::VmIdPathBudgetExceeded`] when the
    /// selected `vm_id` would overflow the AF_UNIX `sun_path` cap.
    /// Does not block.
    pub fn admit(self: &Arc<Self>, mut config: SandboxConfig) -> Result<Sandbox, FcError> {
        validate_caller_boot_args_if_present(config.boot_args.as_deref())?;
        if let Some(workspace) = config.workspace.as_ref() {
            config.workspace = Some(canonicalize_workspace_root(workspace)?);
        }
        validate_huge_pages_memory(&config)?;
        validate_network_policy(&config.network)?;

        let vm_id = config.vm_id.get_or_insert_with(auto_vm_id);
        // Validate the selected vm_id up front so the failure surfaces as a
        // typed config error rather than as an opaque AF_UNIX failure after
        // launch has partially materialized a jail.
        check_vm_id_name(vm_id)?;
        check_vm_id_reserved_name(vm_id)?;
        check_vm_id_path_budget(&self.config, vm_id)?;

        let mut available = self.semaphore.lock().unwrap_or_else(|p| p.into_inner());

        if *available == 0 {
            return Err(FcError::AdmissionRefused {
                limit: self.config.max_concurrent_vms,
            });
        }
        *available -= 1;
        drop(available);

        let permit = AdmissionPermit {
            sem: Arc::clone(&self.semaphore),
            limit: self.config.max_concurrent_vms,
        };

        Ok(Sandbox {
            config,
            permit,
            backend: Arc::clone(self),
            delete_run_dir_on_launch_error: false,
        })
    }

    /// Return the merged effective configuration for diagnostics.
    #[must_use]
    pub fn show_effective_config(&self) -> EffectiveConfig {
        self.effective.clone()
    }

    /// Compute the live snapshot-template input tuple for this backend and sandbox.
    pub fn snapshot_template_inputs(
        &self,
        config: &SandboxConfig,
        hooks: HookSpecSet,
    ) -> Result<TemplateInputs, FcError> {
        crate::warm_pool::template_inputs_for_current_host(self, config, hooks)
    }

    /// Look up or build a snapshot template for this backend and sandbox.
    pub fn build_snapshot_template(
        self: &Arc<Self>,
        config: SandboxConfig,
        hooks: HookSpecSet,
        store: &TemplateStore,
    ) -> Result<PinnedTemplate, FcError> {
        let inputs = self.snapshot_template_inputs(&config, hooks)?;
        crate::warm_pool::build_template(self, inputs, store, &config)
    }

    /// Walk every subdirectory of `<run_root>/` and reap orphaned run-dirs.
    ///
    /// Best-effort: individual failure to reap a specific dir is logged but
    /// does not cause this method to return an error.
    ///
    /// When `force` is true, dirs with ambiguous ownership locks are also
    /// reaped rather than skipped.
    pub fn recover_stale_run_root(&self, force: bool) -> Result<(), FcError> {
        let run_root = &self.config.run_root;
        if !run_root.exists() {
            return Ok(());
        }

        let entries = std::fs::read_dir(run_root).map_err(|source| FcError::PathIo {
            path: run_root.to_path_buf(),
            source,
        })?;
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let subdir = entry.path();
            if !subdir.is_dir() {
                continue;
            }
            if is_reserved_run_root_child(&file_name) {
                continue;
            }
            let Some(name) = file_name.to_str() else {
                warn!(
                    path = %subdir.display(),
                    "recover_stale_run_root: non-UTF-8 run-dir name; preserving"
                );
                continue;
            };
            if !is_valid_vm_id_name(name) {
                warn!(
                    path = %subdir.display(),
                    "recover_stale_run_root: invalid run-dir name; preserving"
                );
                continue;
            }

            // Skip run-dirs that are actively owned or ambiguous. Recovery is
            // destructive, so malformed ownership evidence preserves residue.
            match run_dir_liveness(&subdir) {
                RunDirLiveness::Live => continue,
                RunDirLiveness::Ambiguous => {
                    if force {
                        warn!(
                            path = %subdir.display(),
                            "recover_stale_run_root: ambiguous ownership lock; force-reaping"
                        );
                    } else {
                        warn!(
                            path = %subdir.display(),
                            "recover_stale_run_root: ambiguous ownership lock; preserving run-dir"
                        );
                        continue;
                    }
                }
                RunDirLiveness::Dead => {}
            }

            match inspect_run_dir(&subdir) {
                Ok(m80_jailer::InspectionDecision::LiveJail {
                    jailer_pid,
                    firecracker_pid,
                }) => {
                    // Owner m80 is dead (we passed `run_dir_liveness` above)
                    // but firecracker is still running — orphaned VM.
                    // SIGKILL it, wait for the kernel to reap, then reclaim.
                    tracing::warn!(
                        path = %subdir.display(),
                        jailer_pid,
                        firecracker_pid,
                        "recover_stale_run_root: killing orphaned firecracker"
                    );
                    kill_orphan_pids(jailer_pid, firecracker_pid);
                    self.remove_run_dir(&subdir);
                }
                Ok(m80_jailer::InspectionDecision::OrphanJail { .. }) => {
                    // `reap_plan` from the plan file is ignored —
                    // `remove_run_dir` reads `/proc/self/mountinfo` for the
                    // authoritative mount list, which covers cases the
                    // persisted plan doesn't (partial materialize, older
                    // binary, etc.).
                    self.remove_run_dir(&subdir);
                }
                Ok(m80_jailer::InspectionDecision::NoJail) => {
                    self.remove_run_dir(&subdir);
                }
                Err(e) => {
                    warn!(
                        path = %subdir.display(),
                        err = %e,
                        "recover_stale_run_root: inspect_run_dir failed"
                    );
                }
            }
        }

        Ok(())
    }

    fn remove_run_dir(&self, subdir: &std::path::Path) {
        remove_run_dir_with_network_cleanup(&self.config.run_root, subdir, |vm_id, run_root| {
            self.network_helper.cleanup_vm(vm_id, run_root)
        });
    }

    fn sweep_stale_shared_pmem_refs(&self) -> Result<(), FcError> {
        if !Path::new(DEFAULT_STORE_ROOT).exists() {
            return Ok(());
        }
        let store = ImageStore::open_default()?;
        let live_vm_ids = live_run_dir_names(&self.config.run_root)?;
        store.sweep_shared_refs(live_vm_ids)?;
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ParentCapabilityDrop {
    Run,
    SkipForHostlessTest,
}

fn live_run_dir_names(run_root: &Path) -> Result<Vec<String>, FcError> {
    if !run_root.exists() {
        return Ok(Vec::new());
    }
    let entries = std::fs::read_dir(run_root).map_err(|source| FcError::PathIo {
        path: run_root.to_path_buf(),
        source,
    })?;
    let mut names = Vec::new();
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        if is_reserved_run_root_child(&file_name) {
            continue;
        }
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if is_valid_vm_id_name(name) && entry.path().is_dir() {
            names.push(name.to_owned());
        }
    }
    Ok(names)
}

fn auto_vm_id() -> String {
    let pid = std::process::id();
    let ts = crate::runroot::unix_ms_now();
    format!("vm-{pid}-{ts}")
}

fn validate_huge_pages_memory(config: &SandboxConfig) -> Result<(), FcError> {
    if !config.huge_pages_2m {
        return Ok(());
    }
    let mem_size_mib = config.mem_size_mib.unwrap_or(FIRST_LINE_MEM_SIZE_MIB);
    if mem_size_mib % 2 != 0 {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "huge_pages_2m",
            reason: format!(
                "2 MiB huge pages require mem_size_mib to be a multiple of 2, got {mem_size_mib}"
            ),
        }));
    }
    Ok(())
}

/// Reject a selected `vm_id` whose constructed AF_UNIX socket path would
/// exceed the kernel's `sun_path` cap. The check is purely arithmetic (no
/// filesystem access) so it can run before the admission permit is acquired.
fn check_vm_id_path_budget(config: &BackendConfig, vm_id: &str) -> Result<(), FcError> {
    let fc_basename = config
        .discovery
        .firecracker_bin
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| {
            FcError::Config(ConfigError::InvalidValue {
                field: "firecracker_bin",
                reason: "path contains non-UTF-8 characters".into(),
            })
        })?;
    let path_len = socket_path_len(&config.run_root, vm_id, fc_basename);
    if path_len > SUN_PATH_BUDGET {
        return Err(FcError::Config(ConfigError::VmIdPathBudgetExceeded {
            vm_id: vm_id.to_owned(),
            run_root: config.run_root.clone(),
            fc_basename: fc_basename.to_owned(),
            path_len,
            budget: SUN_PATH_BUDGET,
        }));
    }
    Ok(())
}

fn check_vm_id_name(vm_id: &str) -> Result<(), FcError> {
    if is_valid_vm_id_name(vm_id) {
        return Ok(());
    }
    Err(invalid_vm_id(
        vm_id,
        "must be 1..=64 ASCII alphanumeric, '.', '_', or '-' characters and must not be '.' or '..'",
    ))
}

fn is_valid_vm_id_name(vm_id: &str) -> bool {
    !vm_id.is_empty()
        && vm_id.len() <= 64
        && vm_id != "."
        && vm_id != ".."
        && vm_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn check_vm_id_reserved_name(vm_id: &str) -> Result<(), FcError> {
    if is_reserved_run_root_child(std::ffi::OsStr::new(vm_id)) {
        return Err(invalid_vm_id(
            vm_id,
            format!("{vm_id:?} is reserved under run_root"),
        ));
    }
    Ok(())
}

fn invalid_vm_id(vm_id: &str, reason: impl Into<String>) -> FcError {
    FcError::InvalidVmId {
        vm_id: vm_id.to_owned(),
        reason: reason.into(),
    }
}

fn canonicalize_workspace_root(workspace: &std::path::Path) -> Result<PathBuf, FcError> {
    let metadata = std::fs::symlink_metadata(workspace).map_err(|source| FcError::PathIo {
        path: workspace.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "workspace",
            reason: format!(
                "workspace root {} must not be a symlink",
                workspace.display()
            ),
        }));
    }
    if !metadata.is_dir() {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "workspace",
            reason: format!("workspace root {} must be a directory", workspace.display()),
        }));
    }
    std::fs::canonicalize(workspace).map_err(|source| FcError::PathIo {
        path: workspace.to_path_buf(),
        source,
    })
}

fn validate_network_policy(policy: &crate::NetworkPolicy) -> Result<(), FcError> {
    let crate::NetworkPolicy::JoinNetns { spec } = policy else {
        return Ok(());
    };
    for resolver in &spec.dns_resolvers {
        if !m80_net_outbound::is_admitted_dns_resolver(*resolver) {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "network.join_netns.dns_resolvers",
                reason: format!("resolver {resolver} is not admitted"),
            }));
        }
    }
    Ok(())
}

fn is_reserved_run_root_child(name: &std::ffi::OsStr) -> bool {
    name == ".preserved" || name == "warm"
}

/// Build an `EffectiveConfig` snapshot from a `BackendConfig`.
///
/// All fields are tagged `Default` because `Backend::new` doesn't know which
/// source each field came from — the merge happens in `config::load`. A caller
/// that went through `config::load` already holds a properly-tagged
/// `EffectiveConfig` and can pass it alongside the `BackendConfig`.
fn build_effective_config(cfg: &BackendConfig) -> EffectiveConfig {
    let fields = vec![
        EffectiveField {
            name: "max_concurrent_vms".into(),
            value: cfg.max_concurrent_vms.to_string(),
            source: ConfigSource::Default,
        },
        EffectiveField {
            name: "run_root".into(),
            value: cfg.run_root.display().to_string(),
            source: ConfigSource::Default,
        },
        EffectiveField {
            name: "jail_uid".into(),
            value: cfg.jail_uid.to_string(),
            source: ConfigSource::Default,
        },
        EffectiveField {
            name: "jail_gid".into(),
            value: cfg.jail_gid.to_string(),
            source: ConfigSource::Default,
        },
        EffectiveField {
            name: "cgroup_mode".into(),
            value: match cfg.cgroup_mode {
                CgroupMode::UnifiedV2 => "unified-v2".into(),
                CgroupMode::Disabled => "disabled".into(),
            },
            source: ConfigSource::Default,
        },
    ];
    EffectiveConfig { fields }
}

/// Unmount everything under `subdir` (using mountinfo as ground truth),
/// remove owned network state, remove the cgroup leaf, then `remove_dir_all`.
/// Without the umount pass, a SIGKILL'd m80 leaves bind-mounted kernel +
/// rootfs.ext4 inside the chroot and the rm fails with EBUSY; without network
/// cleanup, the TAP and iptables state outlive the run-dir evidence needed to
/// remove them; without the cgroup rm, the empty leaf in
/// `/sys/fs/cgroup/m80-firecracker/<vm_id>/` stays around forever.
fn remove_run_dir_with_network_cleanup<F>(
    run_root: &std::path::Path,
    subdir: &std::path::Path,
    mut cleanup_network: F,
) where
    F: FnMut(&str, &std::path::Path) -> Result<(), crate::NetworkHelperError>,
{
    unmount_under(subdir);
    match subdir.file_name().and_then(|s| s.to_str()) {
        Some(vm_id) => {
            if subdir.join(m80_net_outbound::NETWORK_STATE_FILE).exists() {
                if let Err(e) = cleanup_network(vm_id, run_root) {
                    warn!(vm_id, err = %e, "recover_stale_run_root: network cleanup failed");
                }
            }
            if let Err(e) = m80_cgroup::cleanup_orphan_subtree(vm_id) {
                warn!(vm_id, err = %e, "recover_stale_run_root: cgroup cleanup failed");
            }
        }
        None => {
            warn!(
                path = %subdir.display(),
                "recover_stale_run_root: non-UTF-8 dir name; skipping cgroup cleanup"
            );
        }
    }
    if let Err(e) = std::fs::remove_dir_all(subdir) {
        warn!(path = %subdir.display(), err = %e, "recover_stale_run_root: remove_dir_all failed");
    } else {
        tracing::info!(path = %subdir.display(), "recover_stale_run_root: reaped orphan run-dir");
    }
}

/// SIGKILL any orphaned jailer / firecracker pid and poll until /proc/<pid>
/// disappears (signaling that PID 1 has reaped it). Best-effort: a stuck
/// or already-dead pid is logged and skipped. The 2-second poll deadline
/// is enough for a normal SIGKILL teardown; longer waits suggest something
/// is wrong and we'd rather move on with the cleanup than hang.
fn kill_orphan_pids(jailer_pid: u32, firecracker_pid: u32) {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;

    // Dedupe: with `--daemonize` jailer fork-execs and the two pids differ;
    // without it (m80's path) jailer execs into firecracker so they're equal.
    let mut pids = vec![firecracker_pid];
    if jailer_pid != 0 && jailer_pid != firecracker_pid {
        pids.push(jailer_pid);
    }

    for pid in &pids {
        match kill(Pid::from_raw(*pid as i32), Some(Signal::SIGKILL)) {
            Ok(()) => {}
            Err(nix::errno::Errno::ESRCH) => {} // already gone
            Err(e) => {
                warn!(pid, err = %e, "recover_stale_run_root: SIGKILL failed");
            }
        }
    }

    // Poll until each /proc/<pid> is gone (init has reaped) or we hit 2s.
    let deadline = Instant::now() + Duration::from_secs(2);
    for pid in &pids {
        let proc_path = std::path::PathBuf::from(format!("/proc/{pid}"));
        while proc_path.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        if proc_path.exists() {
            warn!(
                pid,
                "recover_stale_run_root: pid still present after 2s SIGKILL wait"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::PathBuf;

    use super::*;

    fn fake_discovery(run_root: &std::path::Path) -> m80_preflight::Discovery {
        let rootfs = tempfile::NamedTempFile::new().expect("fake rootfs");
        let rootfs_path = rootfs.path().to_path_buf();
        let rootfs_file = rootfs.reopen().expect("fake rootfs fd");
        let net_helper_bin = fake_net_helper(run_root);
        m80_preflight::Discovery {
            firecracker_bin: PathBuf::from("/tmp/firecracker"),
            firecracker_seccomp_filter: PathBuf::from("/tmp/firecracker-seccomp-filter.bin"),
            jailer_bin: PathBuf::from("/tmp/jailer"),
            firecracker_version: "v1.0.0".to_owned(),
            jailer_version: "v1.0.0".to_owned(),
            jailer_harden_bin: PathBuf::from("/tmp/m80-jailer-harden"),
            net_helper_bin,
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

    fn fake_net_helper(run_root: &std::path::Path) -> PathBuf {
        std::fs::create_dir_all(run_root).expect("run root");
        let path = run_root.join("m80-net-helper-test");
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

    #[test]
    fn startup_recovery_preserves_warm_control_tree() {
        let run_root = tempfile::tempdir().expect("run root");
        let snapshot_dir = run_root.path().join("warm/snapshot");
        std::fs::create_dir_all(&snapshot_dir).expect("snapshot dir");
        let marker = snapshot_dir.join("vm.snap");
        std::fs::write(&marker, b"snapshot").expect("snapshot marker");

        let config = BackendConfig::builder(fake_discovery(run_root.path()))
            .max_concurrent_vms(1)
            .run_root(run_root.path())
            .jail_uid(3000)
            .jail_gid(3000)
            .cgroup_mode(CgroupMode::Disabled)
            .build();
        let _backend = Backend::new(config).expect("Backend::new");

        assert!(
            marker.exists(),
            "warm control/snapshot tree must not be reaped as a stale VM run-dir"
        );
    }

    #[test]
    fn admit_canonicalizes_workspace_root_before_sandbox_creation() {
        let run_root = tempfile::tempdir().expect("run root");
        let workspace = tempfile::tempdir().expect("workspace");
        let backend = test_backend(run_root.path());
        let mut config = SandboxConfig::default();
        config.workspace = Some(workspace.path().join("."));

        let sandbox = backend.admit(config).expect("admit sandbox");

        assert_eq!(
            sandbox.config.workspace.as_deref(),
            Some(
                workspace
                    .path()
                    .canonicalize()
                    .expect("canonical workspace")
                    .as_path()
            )
        );
    }

    #[test]
    fn admit_rejects_symlink_workspace_root() {
        let run_root = tempfile::tempdir().expect("run root");
        let target = tempfile::tempdir().expect("target");
        let backend = test_backend(run_root.path());
        let link = run_root.path().join("workspace-link");
        std::os::unix::fs::symlink(target.path(), &link).expect("workspace symlink");
        let mut config = SandboxConfig::default();
        config.workspace = Some(link);

        let err = backend
            .admit(config)
            .expect_err("symlink workspace root must be rejected");

        assert!(
            matches!(
                err,
                FcError::Config(ConfigError::InvalidValue {
                    field: "workspace",
                    ..
                })
            ),
            "expected workspace config rejection, got {err:?}"
        );
    }

    #[test]
    fn admit_rejects_join_netns_unadmitted_dns_resolver() {
        let run_root = tempfile::tempdir().expect("run root");
        let backend = test_backend(run_root.path());
        let mut config = SandboxConfig::default();
        config.network = join_netns_policy(std::net::Ipv4Addr::new(203, 0, 113, 1));

        let err = backend
            .admit(config)
            .expect_err("unadmitted JoinNetns resolver must fail admission");

        assert!(
            matches!(
                err,
                FcError::Config(ConfigError::InvalidValue {
                    field: "network.join_netns.dns_resolvers",
                    ..
                })
            ),
            "expected dns resolver config rejection, got {err:?}"
        );
    }

    #[test]
    fn admit_accepts_join_netns_admitted_dns_resolver() {
        let run_root = tempfile::tempdir().expect("run root");
        let backend = test_backend(run_root.path());
        let mut config = SandboxConfig::default();
        config.network = join_netns_policy(std::net::Ipv4Addr::new(1, 1, 1, 1));

        backend.admit(config).expect("admitted resolver");
    }

    #[test]
    fn admit_rejects_odd_mem_size_with_huge_pages() {
        let run_root = tempfile::tempdir().expect("run root");
        let backend = test_backend(run_root.path());
        let config = SandboxConfig {
            mem_size_mib: Some(513),
            huge_pages_2m: true,
            ..SandboxConfig::default()
        };

        let err = backend
            .admit(config)
            .expect_err("hugetlbfs memory must be 2 MiB aligned");

        assert!(
            matches!(
                err,
                FcError::Config(ConfigError::InvalidValue {
                    field: "huge_pages_2m",
                    ..
                })
            ),
            "expected huge_pages_2m config rejection, got {err:?}"
        );
    }

    #[test]
    fn remove_run_dir_cleans_network_before_deleting_state() {
        let run_root = tempfile::tempdir().expect("run root");
        let subdir = run_root.path().join("vm-net");
        std::fs::create_dir_all(&subdir).expect("run dir");
        let state_path = subdir.join(m80_net_outbound::NETWORK_STATE_FILE);
        std::fs::write(&state_path, b"{}").expect("network state");

        let calls = RefCell::new(Vec::new());
        remove_run_dir_with_network_cleanup(run_root.path(), &subdir, |vm_id, cleanup_root| {
            assert_eq!(vm_id, "vm-net");
            assert_eq!(cleanup_root, run_root.path());
            assert!(
                state_path.exists(),
                "network cleanup must run before run-dir deletion removes network-state.json"
            );
            calls.borrow_mut().push(vm_id.to_owned());
            Ok::<(), crate::NetworkHelperError>(())
        });

        assert_eq!(calls.into_inner(), vec!["vm-net"]);
        assert!(!subdir.exists());
    }

    #[test]
    fn remove_run_dir_still_reaps_when_network_cleanup_fails() {
        let run_root = tempfile::tempdir().expect("run root");
        let subdir = run_root.path().join("vm-net-fail");
        std::fs::create_dir_all(&subdir).expect("run dir");
        std::fs::write(subdir.join(m80_net_outbound::NETWORK_STATE_FILE), b"{}")
            .expect("network state");

        remove_run_dir_with_network_cleanup(run_root.path(), &subdir, |_vm_id, _cleanup_root| {
            Err(crate::NetworkHelperError::OperationFailed {
                operation: crate::NetworkHelperOperation::CleanupVm,
                kind: m80_net_outbound::NetworkHelperFailureKind::OperationFailed,
                detail: "synthetic cleanup failure".to_owned(),
            })
        });

        assert!(!subdir.exists());
    }

    fn test_backend(run_root: &std::path::Path) -> Arc<Backend> {
        Arc::new(
            Backend::new(
                BackendConfig::builder(fake_discovery(run_root))
                    .max_concurrent_vms(1)
                    .run_root(run_root)
                    .jail_uid(3000)
                    .jail_gid(3000)
                    .cgroup_mode(CgroupMode::Disabled)
                    .build(),
            )
            .expect("backend"),
        )
    }

    fn join_netns_policy(resolver: std::net::Ipv4Addr) -> crate::NetworkPolicy {
        crate::NetworkPolicy::JoinNetns {
            spec: crate::NetnsSpec {
                netns_path: "/proc/self/ns/net".into(),
                tap_name: "tap0".to_owned(),
                guest_mac: crate::MacAddr::parse("02:00:00:00:00:01").expect("mac"),
                guest_ipv4: "10.80.0.2/24".parse().expect("guest ipv4"),
                gateway_ipv4: "10.80.0.1".parse().expect("gateway"),
                dns_resolvers: vec![resolver],
            },
        }
    }
}

/// Read `/proc/self/mountinfo` and `umount2(MNT_DETACH)` every mountpoint
/// that is `root` itself or lives under it. Deepest first so nested mounts
/// unwind cleanly. Best-effort: failures are logged but don't abort.
fn unmount_under(root: &std::path::Path) {
    use nix::mount::{umount2, MntFlags};

    let Ok(mountinfo) = std::fs::read_to_string("/proc/self/mountinfo") else {
        return;
    };

    // Field 5 of each line is the mountpoint (per `proc(5)` mountinfo).
    let mut targets: Vec<std::path::PathBuf> = mountinfo
        .lines()
        .filter_map(|line| line.split_whitespace().nth(4).map(std::path::PathBuf::from))
        .filter(|mp| mp == root || mp.starts_with(root))
        .collect();

    // Deepest first.
    targets.sort_by_key(|p| std::cmp::Reverse(p.components().count()));

    for mp in targets {
        if let Err(e) = umount2(&mp, MntFlags::MNT_DETACH) {
            warn!(path = %mp.display(), err = %e, "recover_stale_run_root: umount2 failed");
        }
    }
}
