//! [`Backend`] implementation: construction, admission, effective-config query,
//! and stale run-root recovery.

use std::sync::{Arc, Condvar, Mutex};

use tracing::warn;

use m80_jailer::recover_from_run_dir;

use crate::error::FcError;
use crate::runroot::run_dir_is_live;
use crate::types::{
    AdmissionPermit, Backend, BackendConfig, CgroupMode, ConfigSource, EffectiveConfig,
    EffectiveField, Sandbox, SandboxConfig,
};

impl Backend {
    /// Construct a backend handle.
    ///
    /// `config.discovery` must already be populated by the caller from
    /// `m80-preflight::run()`. No preflight is re-run here.
    pub fn new(config: BackendConfig) -> Result<Self, FcError> {
        let permits = config.max_concurrent_vms;
        let semaphore = Arc::new((Mutex::new(permits), Condvar::new()));
        let effective = build_effective_config(&config);
        Ok(Backend { config, effective, semaphore })
    }

    /// Acquire one admission permit and return a [`Sandbox`] in Created state.
    ///
    /// Fails fast with [`FcError::AdmissionRefused`] when no permits are
    /// available. Does not block.
    pub fn admit(self: &Arc<Self>, config: SandboxConfig) -> Result<Sandbox, FcError> {
        let (lock, _cvar) = self.semaphore.as_ref();
        let mut available = lock.lock().unwrap_or_else(|p| p.into_inner());

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
        })
    }

    /// Return the merged effective configuration for diagnostics.
    pub fn show_effective_config(&self) -> EffectiveConfig {
        self.effective.clone()
    }

    /// Walk every subdirectory of `<run_root>/` and reap orphaned run-dirs.
    ///
    /// Best-effort: individual failure to reap a specific dir is logged but
    /// does not cause this method to return an error.
    pub fn recover_stale_run_root(&self) -> Result<(), FcError> {
        let run_root = &self.config.run_root;
        if !run_root.exists() {
            return Ok(());
        }

        let entries = std::fs::read_dir(run_root)?;
        for entry in entries.flatten() {
            let subdir = entry.path();
            if !subdir.is_dir() {
                continue;
            }

            // Skip run-dirs that are actively owned by a live process.
            if run_dir_is_live(&subdir) {
                continue;
            }

            match recover_from_run_dir(&subdir) {
                Ok(m80_jailer::RecoveryDecision::LiveJail { jailer_pid, firecracker_pid }) => {
                    tracing::debug!(
                        path = %subdir.display(),
                        jailer_pid,
                        firecracker_pid,
                        "recover_stale_run_root: live jail found, skipping"
                    );
                }
                Ok(m80_jailer::RecoveryDecision::OrphanJail { reap_steps }) => {
                    reap_orphan_run_dir(&subdir, &reap_steps);
                }
                Ok(m80_jailer::RecoveryDecision::NoJail) => {
                    remove_run_dir(&subdir);
                }
                Err(e) => {
                    warn!(
                        path = %subdir.display(),
                        err = %e,
                        "recover_stale_run_root: recover_from_run_dir failed"
                    );
                }
            }
        }

        Ok(())
    }
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

/// Reap an orphan run-dir.
///
/// `_reap_steps` is unused: `remove_run_dir` now reads `/proc/self/mountinfo`
/// to find any bind mounts under the dir and unmounts them via the kernel's
/// authoritative view. The persisted plan can be partial (m80 SIGKILL'd
/// mid-materialize, the plan file is from an older binary, etc.); trusting
/// mountinfo means we recover from cases the plan file doesn't describe.
fn reap_orphan_run_dir(subdir: &std::path::Path, _reap_steps: &[m80_jailer::PlanStep]) {
    remove_run_dir(subdir);
}

/// Unmount everything under `subdir` (using mountinfo as ground truth) then
/// `remove_dir_all`. Without the umount pass, a SIGKILL'd m80 leaves
/// bind-mounted kernel + rootfs.ext4 inside the chroot, and the rm fails
/// with EBUSY.
fn remove_run_dir(subdir: &std::path::Path) {
    unmount_under(subdir);
    if let Err(e) = std::fs::remove_dir_all(subdir) {
        warn!(path = %subdir.display(), err = %e, "recover_stale_run_root: remove_dir_all failed");
    } else {
        tracing::info!(path = %subdir.display(), "recover_stale_run_root: reaped orphan run-dir");
    }
}

/// Read `/proc/self/mountinfo` and `umount2(MNT_DETACH)` every mountpoint
/// that is `root` itself or lives under it. Deepest first so nested mounts
/// unwind cleanly. Best-effort: failures are logged but don't abort.
fn unmount_under(root: &std::path::Path) {
    use nix::mount::{MntFlags, umount2};

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
