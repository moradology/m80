//! Integration tests requiring root and a real cgroup v2 host.
//!
//! All tests here are `#[ignore]` — they require:
//! - Running as root (or with `CAP_SYS_ADMIN`).
//! - A host with unified cgroup v2 at `/sys/fs/cgroup`.
//! - A real `jailer` + `firecracker` binary available.
//!
//! Run with:
//!   sudo cargo test -p m80-cgroup -- --ignored
//!
//! These tests document the real-cgroup test path; they are not expected to
//! run in CI without a privileged cgroup v2 environment.

/// Probe returns Ok on a real unified-v2 host.
///
/// Verifies that `Subtree::probe()` does not error on a host that has
/// `cgroup2` mounted at `/sys/fs/cgroup`.
#[test]
#[ignore]
fn probe_returns_ok_on_unified_v2_host() {
    m80_cgroup::Subtree::probe().expect("probe() must return Ok on a unified-v2 host");
}

/// Subtree creation places the directory under the expected path.
///
/// Would verify:
/// - `/sys/fs/cgroup/m80-firecracker/<vm_id>/` exists after `create`.
/// - `cpu.max`, `memory.max`, `pids.max` are writable.
/// - Both jailer pid and firecracker pid appear in `cgroup.procs`.
/// - `<run_dir>/cgroup-path.txt` contains the absolute subtree path.
#[test]
#[ignore]
fn subtree_creation_places_leaf_under_m80_firecracker() {
    // Requires a real MaterializedJail + JailedFirecracker from m80-jailer.
    // Documented here to describe the integration test shape.
    unimplemented!("requires root + real jailer: see task description");
}

/// Drop removes the subtree if no processes remain.
///
/// Would verify:
/// - After dropping `Subtree`, the directory is gone.
/// - If PIDs remain, the directory stays and a warning is emitted (no panic).
#[test]
#[ignore]
fn drop_removes_empty_subtree() {
    unimplemented!("requires root + real cgroup v2 host");
}

/// `cleanup_orphan_subtree` removes a stale empty subtree.
///
/// Would verify:
/// - A manually-created dir under `m80-firecracker/` with no pids is removed.
/// - A dir with live pids is left in place.
/// - A non-existent path returns `Ok`.
#[test]
#[ignore]
fn orphan_cleanup_removes_empty_stale_subtree() {
    unimplemented!("requires root + real cgroup v2 host");
}

/// `apply_limits` writes controller files correctly.
///
/// Would verify each of:
/// - `cpu.max` written as `"<quota> <period>"`.
/// - `memory.max` written as `"<bytes>"`.
/// - `pids.max` written as `"<count>"`.
/// - `None` fields leave existing values unchanged.
#[test]
#[ignore]
fn apply_limits_writes_controller_files() {
    unimplemented!("requires root + real cgroup v2 host");
}
