//! [`StoppedSandbox`] method implementations.

use std::path::{Path, PathBuf};

use m80_observability::Phase;
use m80_storage::ChangeSet;

use crate::error::{ConfigError, FcError};
use crate::runroot::unix_ms_now;
use crate::types::StoppedSandbox;

impl StoppedSandbox {
    /// Return the per-VM run directory.
    #[must_use]
    pub fn run_dir(&self) -> &Path {
        &self.run_dir
    }

    /// Opt-in change extraction from the workspace scratch image.
    ///
    /// Returns `FcError::Config` if no workspace (scratch image) was
    /// configured for this sandbox.
    pub fn extract_changes(&self, into: &Path) -> Result<ChangeSet, FcError> {
        let scratch = self
            .scratch
            .as_ref()
            .ok_or_else(|| FcError::Config(ConfigError::MissingField { field: "workspace" }))?;
        let max_extract_bytes = std::fs::metadata(scratch.path())
            .map_err(|source| {
                FcError::Storage(m80_storage::StorageError::Io {
                    path: scratch.path().to_path_buf(),
                    source,
                })
            })?
            .len();
        let cs = m80_storage::Scratch::extract(scratch.path(), into, Some(max_extract_bytes))?;
        Ok(cs)
    }

    /// Remove the per-VM run-dir and release the admission permit.
    pub fn delete(mut self) -> Result<(), FcError> {
        crate::diagnostics::record_owned(
            &mut self.diagnostics,
            Phase::Delete,
            &self.vm_id,
            self.request_id.as_deref(),
            "delete started",
        );
        self.cleanup_outbound_network_if_needed()?;
        match std::fs::remove_dir_all(&self.run_dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(FcError::PathIo {
                    path: self.run_dir.clone(),
                    source,
                });
            }
        }
        // `self` drops here; AdmissionPermit::drop returns the slot.
        Ok(())
    }

    /// Move the per-VM run-dir to `.preserved/<unix_ms>-<vm_id>/` for
    /// offline triage. The admission permit is released. Returns the new path.
    pub fn preserve_for_triage(mut self) -> Result<PathBuf, FcError> {
        let preserved_parent = self.run_root.join(".preserved");
        std::fs::create_dir_all(&preserved_parent).map_err(|source| FcError::PathIo {
            path: preserved_parent.clone(),
            source,
        })?;

        let ts = unix_ms_now();
        let dest = preserved_parent.join(format!("{ts}-{}", self.vm_id));
        crate::diagnostics::record_owned(
            &mut self.diagnostics,
            Phase::Delete,
            &self.vm_id,
            self.request_id.as_deref(),
            "preserve for triage",
        );
        self.cleanup_outbound_network_if_needed()?;
        std::fs::rename(&self.run_dir, &dest).map_err(|source| FcError::PathIo {
            path: dest.clone(),
            source,
        })?;

        // `self` drops here; permit returned.
        Ok(dest)
    }

    fn cleanup_outbound_network_if_needed(&mut self) -> Result<(), FcError> {
        let state_path = self.run_dir.join(m80_net_outbound::NETWORK_STATE_FILE);
        if !self.network_cleanup && !state_path.exists() {
            return Ok(());
        }
        crate::diagnostics::record_owned(
            &mut self.diagnostics,
            Phase::Delete,
            &self.vm_id,
            self.request_id.as_deref(),
            "outbound network cleanup started",
        );
        self.network_helper
            .cleanup_vm(&self.vm_id, &self.run_root)?;
        self.network_cleanup = false;
        crate::diagnostics::record_owned(
            &mut self.diagnostics,
            Phase::Delete,
            &self.vm_id,
            self.request_id.as_deref(),
            "outbound network cleanup complete",
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt as _;
    use std::sync::{Arc, Mutex};

    use crate::network_helper::NetworkHelperClient;
    use crate::runroot::write_ownership_lock;
    use crate::types::{AdmissionPermit, StoppedSandbox};

    fn stopped_sandbox(run_root: &std::path::Path, vm_id: &str) -> StoppedSandbox {
        let semaphore = Arc::new(Mutex::new(0u32));
        let run_dir = run_root.join(vm_id);
        std::fs::create_dir_all(&run_dir).unwrap();
        let lease_guard = write_ownership_lock(&run_dir).unwrap();
        StoppedSandbox {
            vm_id: vm_id.to_string(),
            request_id: None,
            run_dir,
            scratch: None,
            permit: AdmissionPermit {
                sem: semaphore,
                limit: 1,
            },
            lease_guard,
            run_root: run_root.to_path_buf(),
            diagnostics: None,
            network_cleanup: false,
            network_helper: Arc::new(NetworkHelperClient::new("/tmp/m80-net-helper".into())),
        }
    }

    fn write_executable(path: &std::path::Path, content: &str) {
        let mut file = std::fs::File::create(path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file.sync_all().unwrap();
        drop(file);
        let mut perms = std::fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).unwrap();
    }

    #[test]
    fn delete_removes_entire_run_dir() {
        let dir = tempfile::tempdir().unwrap();
        let run_root = dir.path();
        let sandbox = stopped_sandbox(run_root, "vm-delete");
        let run_dir = sandbox.run_dir().to_path_buf();
        std::fs::create_dir_all(run_dir.join("nested")).unwrap();
        std::fs::write(run_dir.join("nested/file.txt"), b"state").unwrap();

        sandbox.delete().unwrap();

        assert!(!run_dir.exists());
    }

    #[test]
    fn delete_tolerates_already_missing_run_dir() {
        let dir = tempfile::tempdir().unwrap();
        let run_root = dir.path();
        let sandbox = stopped_sandbox(run_root, "vm-delete-missing");
        std::fs::remove_dir_all(sandbox.run_dir()).unwrap();

        sandbox.delete().unwrap();
    }

    #[test]
    fn delete_routes_outbound_cleanup_through_network_helper() {
        let dir = tempfile::tempdir().unwrap();
        let run_root = dir.path();
        let log = run_root.join("requests.log");
        let helper_path = run_root.join("helper.sh");
        write_executable(
            &helper_path,
            &format!(
                r#"#!/bin/sh
while IFS= read -r line; do
  printf '%s\n' "$line" >> "{}"
  printf '%s\n' '{{"status":"ok","success":{{"kind":"empty"}}}}'
done
"#,
                log.display()
            ),
        );
        let mut sandbox = stopped_sandbox(run_root, "vm-delete-net");
        std::fs::write(
            sandbox.run_dir().join(m80_net_outbound::NETWORK_STATE_FILE),
            b"{}",
        )
        .unwrap();
        sandbox.network_cleanup = true;
        sandbox.network_helper = Arc::new(NetworkHelperClient::new(helper_path));

        sandbox.delete().unwrap();

        let requests = std::fs::read_to_string(log).unwrap();
        assert!(requests.contains(r#""op":"cleanup_vm""#));
        assert!(requests.contains(r#""vm_id":"vm-delete-net""#));
    }

    #[test]
    fn preserve_for_triage_moves_run_dir_under_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let run_root = dir.path();
        let sandbox = stopped_sandbox(run_root, "vm-preserve");
        let run_dir = sandbox.run_dir().to_path_buf();
        std::fs::create_dir_all(&run_dir).unwrap();
        std::fs::write(run_dir.join("state.txt"), b"state").unwrap();

        let preserved = sandbox.preserve_for_triage().unwrap();

        assert!(!run_dir.exists());
        assert!(preserved.starts_with(run_root.join(".preserved")));
        assert!(preserved
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with("-vm-preserve"));
        assert_eq!(
            std::fs::read(preserved.join("state.txt")).unwrap(),
            b"state"
        );
    }
}
