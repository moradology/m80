//! Integration tests requiring root / CAP_SYS_ADMIN.
//!
//! All tests in this file are marked `#[ignore]` — they require real mount
//! privileges and are not expected to run in CI. Run manually as root:
//!
//! ```sh
//! sudo cargo test -p m80-jailer -- --ignored
//! ```

use m80_jailer::{BindMode, Binding, JailerConfig, Plan};
use std::path::PathBuf;

/// Documents materialize behaviour: CreateDir and Bind steps are executed via
/// real syscalls, and `jailer-plan.json` + `jailer-state.json` are written to
/// the run-dir.
#[test]
#[ignore = "requires CAP_SYS_ADMIN / root"]
fn materialize_creates_jail_root_and_persists_plan() {
    let run_dir = tempfile::tempdir().unwrap();
    let kernel_file = tempfile::NamedTempFile::new().unwrap();

    let cfg = JailerConfig {
        jailer_bin: PathBuf::from("/usr/bin/jailer"),
        firecracker_bin: PathBuf::from("/usr/bin/firecracker"),
        run_dir: run_dir.path().to_path_buf(),
        uid: 3000,
        gid: 3000,
        bindings: vec![Binding {
            source: kernel_file.path().to_path_buf(),
            dest: PathBuf::from("kernel/vmlinux"),
            mode: BindMode::Ro,
        }],
        sockets: Vec::new(),
        stdio_log: None,
    };

    let plan = Plan::compute(&cfg).unwrap();
    let jail = plan
        .materialize()
        .expect("materialize must succeed as root");

    assert!(
        jail.jail_path.exists(),
        "jail root must exist after materialize"
    );
    assert!(
        run_dir.path().join("jailer-plan.json").exists(),
        "jailer-plan.json must be written"
    );
    assert!(
        run_dir.path().join("jailer-state.json").exists(),
        "jailer-state.json must be written"
    );
}
