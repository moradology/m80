//! [`StoppedSandbox`] method implementations.

use std::path::{Path, PathBuf};

use m80_observability::Phase;
use m80_storage::ChangeSet;

use crate::error::{ConfigError, FcError};
use crate::runroot::unix_ms_now;
use crate::types::StoppedSandbox;

impl StoppedSandbox {
    /// Return the per-VM run directory.
    pub fn run_dir(&self) -> &Path {
        &self.run_dir
    }

    /// Opt-in change extraction from the workspace scratch image.
    ///
    /// Returns `FcError::Config` if no workspace (scratch image) was
    /// configured for this sandbox.
    pub fn extract_changes(&self, into: &Path) -> Result<ChangeSet, FcError> {
        let scratch = self.scratch.as_ref().ok_or_else(|| {
            FcError::Config(ConfigError::MissingField { field: "workspace" })
        })?;
        let cs = m80_storage::Scratch::extract(scratch.path(), into)?;
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
        match std::fs::remove_dir_all(&self.run_dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        // `self` drops here; AdmissionPermit::drop returns the slot.
        Ok(())
    }

    /// Move the per-VM run-dir to `.preserved/<unix_ms>-<vm_id>/` for
    /// offline triage. The admission permit is released. Returns the new path.
    pub fn preserve_for_triage(mut self) -> Result<PathBuf, FcError> {
        let preserved_parent = self.run_root.join(".preserved");
        std::fs::create_dir_all(&preserved_parent)?;

        let ts = unix_ms_now();
        let dest = preserved_parent.join(format!("{ts}-{}", self.vm_id));
        crate::diagnostics::record_owned(
            &mut self.diagnostics,
            Phase::Delete,
            &self.vm_id,
            self.request_id.as_deref(),
            "preserve for triage",
        );
        std::fs::rename(&self.run_dir, &dest)?;

        // `self` drops here; permit returned.
        Ok(dest)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Condvar, Mutex};

    use crate::types::{AdmissionPermit, StoppedSandbox};

    fn stopped_sandbox(run_root: &std::path::Path, vm_id: &str) -> StoppedSandbox {
        let semaphore = Arc::new((Mutex::new(0), Condvar::new()));
        StoppedSandbox {
            vm_id: vm_id.to_string(),
            request_id: None,
            run_dir: run_root.join(vm_id),
            scratch: None,
            permit: AdmissionPermit {
                sem: semaphore,
                limit: 1,
            },
            run_root: run_root.to_path_buf(),
            diagnostics: None,
        }
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

        sandbox.delete().unwrap();
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
