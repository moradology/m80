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

/// Reap bind-mounts and dirs from an orphan run-dir, then remove it.
fn reap_orphan_run_dir(subdir: &std::path::Path, reap_steps: &[m80_jailer::PlanStep]) {
    use nix::mount::{MntFlags, umount2};

    // Walk reap steps in reverse (plan steps were in creation order).
    for step in reap_steps.iter().rev() {
        match step {
            m80_jailer::PlanStep::Bind { dest, .. } => {
                if let Err(e) = umount2(dest.as_path(), MntFlags::MNT_DETACH) {
                    warn!(
                        path = %dest.display(),
                        err = %e,
                        "recover_stale_run_root: umount2 failed"
                    );
                }
            }
            m80_jailer::PlanStep::CreateDir { path, .. } => {
                if let Err(e) = std::fs::remove_dir_all(path) {
                    warn!(
                        path = %path.display(),
                        err = %e,
                        "recover_stale_run_root: remove_dir_all failed"
                    );
                }
            }
            m80_jailer::PlanStep::Socket { .. } => {
                // Socket files inside the jail are removed when the dir goes.
            }
        }
    }

    remove_run_dir(subdir);
}

/// Remove a run-dir, logging on failure.
fn remove_run_dir(subdir: &std::path::Path) {
    if let Err(e) = std::fs::remove_dir_all(subdir) {
        warn!(path = %subdir.display(), err = %e, "recover_stale_run_root: remove_dir_all failed");
    } else {
        tracing::info!(path = %subdir.display(), "recover_stale_run_root: reaped orphan run-dir");
    }
}
