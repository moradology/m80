use std::path::PathBuf;
use std::sync::Arc;

use crate::network_helper::NetworkHelperClient;

pub(super) struct LaunchRunDirCleanupGuard {
    vm_id: String,
    run_dir: PathBuf,
    armed: bool,
}

impl LaunchRunDirCleanupGuard {
    pub(super) fn new(vm_id: &str, run_dir: PathBuf) -> Self {
        Self {
            vm_id: vm_id.to_owned(),
            run_dir,
            armed: true,
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
