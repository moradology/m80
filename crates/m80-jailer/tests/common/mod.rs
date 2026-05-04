//! Shared test fixtures for m80-jailer integration tests.

use std::path::{Path, PathBuf};

use m80_jailer::JailerConfig;

/// Minimal `JailerConfig` (uid/gid 3000, no bindings, no sockets). Tests
/// extend this with their own bindings/sockets as needed.
pub fn minimal_config(run_dir: &Path) -> JailerConfig {
    JailerConfig {
        jailer_bin: PathBuf::from("/usr/bin/jailer"),
        firecracker_bin: PathBuf::from("/usr/bin/firecracker"),
        run_dir: run_dir.to_path_buf(),
        uid: 3000,
        gid: 3000,
        bindings: Vec::new(),
        sockets: Vec::new(),
    }
}
