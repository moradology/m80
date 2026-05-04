//! [`Backend`] implementation: construction, admission, effective-config query,
//! and stale run-root recovery.

use std::sync::{Arc, Condvar, Mutex};

use std::time::{Duration, Instant};

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
                    // Owner m80 is dead (we passed `run_dir_is_live` above)
                    // but firecracker is still running — orphaned VM.
                    // SIGKILL it, wait for the kernel to reap, then reclaim.
                    tracing::info!(
                        path = %subdir.display(),
                        jailer_pid,
                        firecracker_pid,
                        "recover_stale_run_root: killing orphaned firecracker"
                    );
                    kill_orphan_pids(jailer_pid, firecracker_pid);
                    remove_run_dir(&subdir);
                }
                Ok(m80_jailer::RecoveryDecision::OrphanJail { .. }) => {
                    // `reap_steps` from the plan file is ignored —
                    // `remove_run_dir` reads `/proc/self/mountinfo` for the
                    // authoritative mount list, which covers cases the
                    // persisted plan doesn't (partial materialize, older
                    // binary, etc.).
                    remove_run_dir(&subdir);
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

/// Unmount everything under `subdir` (using mountinfo as ground truth),
/// remove the cgroup leaf, then `remove_dir_all`. Without the umount pass,
/// a SIGKILL'd m80 leaves bind-mounted kernel + rootfs.ext4 inside the
/// chroot and the rm fails with EBUSY; without the cgroup rm, the empty
/// leaf in `/sys/fs/cgroup/m80-firecracker/<vm_id>/` stays around forever.
fn remove_run_dir(subdir: &std::path::Path) {
    unmount_under(subdir);
    if let Some(vm_id) = subdir.file_name().and_then(|s| s.to_str()) {
        if let Err(e) = m80_cgroup::cleanup_orphan_subtree(vm_id) {
            warn!(vm_id, err = %e, "recover_stale_run_root: cgroup cleanup failed");
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
            warn!(pid, "recover_stale_run_root: pid still present after 2s SIGKILL wait");
        }
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
