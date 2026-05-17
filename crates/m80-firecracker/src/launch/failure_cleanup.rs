use std::path::PathBuf;
use std::sync::Arc;

use crate::network_helper::NetworkHelperClient;

pub(super) struct LaunchRunDirCleanupGuard {
    vm_id: String,
    run_dir: PathBuf,
    armed: bool,
    delete_on_error: bool,
}

impl LaunchRunDirCleanupGuard {
    pub(super) fn new(vm_id: &str, run_dir: PathBuf, delete_on_error: bool) -> Self {
        Self {
            vm_id: vm_id.to_owned(),
            run_dir,
            armed: true,
            delete_on_error,
        }
    }

    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for LaunchRunDirCleanupGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if !self.run_dir.exists() {
            return;
        }
        if !self.delete_on_error {
            preserve_failed_launch_run_dir(&self.vm_id, &self.run_dir);
            return;
        }
        match std::fs::remove_dir_all(&self.run_dir) {
            Ok(()) => {
                tracing::warn!(
                    vm_id = %self.vm_id,
                    path = %self.run_dir.display(),
                    "launch failure cleanup removed partial run-dir"
                );
            }
            // Best-effort cleanup in Drop; NotFound is normal when launch failed
            // before the run-dir was created.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::error!(
                    vm_id = %self.vm_id,
                    path = %self.run_dir.display(),
                    error = %e,
                    "launch failure cleanup failed to remove partial run-dir"
                );
            }
        }
    }
}

fn preserve_failed_launch_run_dir(vm_id: &str, run_dir: &std::path::Path) {
    let Some(run_root) = run_dir.parent() else {
        tracing::error!(
            vm_id,
            path = %run_dir.display(),
            "launch failure cleanup could not preserve run-dir without parent"
        );
        return;
    };
    let preserved_parent = run_root.join(".preserved");
    if let Err(e) = std::fs::create_dir_all(&preserved_parent) {
        tracing::error!(
            vm_id,
            path = %preserved_parent.display(),
            error = %e,
            "launch failure cleanup failed to create preserved directory"
        );
        return;
    }
    let dest = preserved_parent.join(format!("{}-{vm_id}", crate::runroot::unix_ms_now()));
    match std::fs::rename(run_dir, &dest) {
        Ok(()) => tracing::warn!(
            vm_id,
            path = %dest.display(),
            "launch failure cleanup preserved partial run-dir"
        ),
        Err(e) => tracing::error!(
            vm_id,
            from = %run_dir.display(),
            to = %dest.display(),
            error = %e,
            "launch failure cleanup failed to preserve partial run-dir"
        ),
    }
}

pub(super) struct LaunchProcessCleanupGuard {
    vm_id: String,
    firecracker_pid: u32,
    jailer_pid: u32,
    armed: bool,
}

pub(super) struct LaunchNetworkCleanupGuard {
    vm_id: String,
    run_root: PathBuf,
    network_helper: Arc<NetworkHelperClient>,
    armed: bool,
}

impl LaunchNetworkCleanupGuard {
    pub(super) fn new(
        network_helper: &Arc<NetworkHelperClient>,
        vm_id: &str,
        run_root: PathBuf,
    ) -> Self {
        Self {
            vm_id: vm_id.to_owned(),
            run_root,
            network_helper: Arc::clone(network_helper),
            armed: true,
        }
    }

    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for LaunchNetworkCleanupGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        match self.network_helper.cleanup_vm(&self.vm_id, &self.run_root) {
            Ok(()) => {
                tracing::warn!(
                    vm_id = %self.vm_id,
                    run_root = %self.run_root.display(),
                    "launch failure cleanup removed outbound network residue"
                );
            }
            Err(e) => {
                tracing::error!(
                    vm_id = %self.vm_id,
                    run_root = %self.run_root.display(),
                    error = %e,
                    "launch failure cleanup failed to remove outbound network residue"
                );
            }
        }
    }
}

impl LaunchProcessCleanupGuard {
    pub(super) fn from_jailed(vm_id: &str, jailed: &m80_jailer::JailedFirecracker) -> Self {
        Self::new(vm_id, jailed.firecracker_pid(), jailed.jailer_pid())
    }

    pub(super) fn new(vm_id: &str, firecracker_pid: u32, jailer_pid: u32) -> Self {
        Self {
            vm_id: vm_id.to_owned(),
            firecracker_pid,
            jailer_pid,
            armed: true,
        }
    }

    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for LaunchProcessCleanupGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        tracing::warn!(
            vm_id = %self.vm_id,
            firecracker_pid = self.firecracker_pid,
            jailer_pid = self.jailer_pid,
            "launch failure cleanup force-killing pre-running sandbox process"
        );
        if let Err(e) = crate::lifecycle::kill_and_reap_pid(self.firecracker_pid) {
            tracing::error!(
                vm_id = %self.vm_id,
                pid = self.firecracker_pid,
                error = %e,
                "launch failure cleanup failed to kill firecracker"
            );
        }
        if self.jailer_pid != self.firecracker_pid {
            if let Err(e) = crate::lifecycle::kill_and_reap_pid(self.jailer_pid) {
                tracing::error!(
                    vm_id = %self.vm_id,
                    pid = self.jailer_pid,
                    error = %e,
                    "launch failure cleanup failed to kill jailer"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_launch_guard_preserves_run_dir_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let run_dir = dir.path().join("vm-preserve");
        std::fs::create_dir(&run_dir).unwrap();
        std::fs::write(run_dir.join("failure_summary.json"), b"{}").unwrap();

        drop(LaunchRunDirCleanupGuard::new(
            "vm-preserve",
            run_dir.clone(),
            false,
        ));

        assert!(!run_dir.exists());
        let preserved_parent = dir.path().join(".preserved");
        let preserved: Vec<_> = std::fs::read_dir(&preserved_parent)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(preserved.len(), 1);
        assert!(preserved[0].join("failure_summary.json").exists());
    }

    #[test]
    fn failed_launch_guard_deletes_when_requested() {
        let dir = tempfile::tempdir().unwrap();
        let run_dir = dir.path().join("vm-delete");
        std::fs::create_dir(&run_dir).unwrap();
        std::fs::write(run_dir.join("failure_summary.json"), b"{}").unwrap();

        drop(LaunchRunDirCleanupGuard::new(
            "vm-delete",
            run_dir.clone(),
            true,
        ));

        assert!(!run_dir.exists());
        assert!(!dir.path().join(".preserved").exists());
    }
}
