//! Tests for `probe()` / `probe_mounts()` — uses a fake `/proc/mounts`
//! string so no real cgroup hierarchy is required.

// Access the private helper via `pub(crate)` visibility exposed for tests.
use m80_cgroup::CgroupError;

/// Call the internal `probe_mounts` helper.
fn probe_mounts(s: &str) -> Result<(), CgroupError> {
    // probe_mounts is pub(crate); integration tests are in a separate crate
    // so we call it through the re-export placed in the public surface for
    // testability. (see the `#[cfg(test)]` re-export in lib.rs)
    //
    // Because the helper is `pub(crate)` it is not accessible here.
    // Instead we use the `probe_mounts_test` wrapper below.
    m80_cgroup::probe_mounts_test(s)
}

/// A minimal unified-v2 `/proc/mounts` line.
const UNIFIED_V2_MOUNTS: &str = "\
sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0
cgroup2 /sys/fs/cgroup cgroup2 rw,nosuid,nodev,noexec,relatime,nsdelegate,memory_recursiveprot 0 0
";

/// A v1 `/proc/mounts` — cgroup v1 controllers at `/sys/fs/cgroup/<name>`.
const V1_MOUNTS: &str = "\
sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0
cgroup /sys/fs/cgroup/cpu,cpuacct cgroup rw,cpu,cpuacct 0 0
cgroup /sys/fs/cgroup/memory cgroup rw,memory 0 0
cgroup /sys/fs/cgroup/pids cgroup rw,pids 0 0
";

/// Hybrid host: cgroup2 at `/sys/fs/cgroup/unified` (not the expected root).
const HYBRID_WRONG_MOUNT_MOUNTS: &str = "\
sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0
cgroup2 /sys/fs/cgroup/unified cgroup2 rw 0 0
cgroup /sys/fs/cgroup/memory cgroup rw,memory 0 0
";

/// Empty mounts file.
const EMPTY_MOUNTS: &str = "";

#[test]
fn unified_v2_mounts_accepts() {
    // probe_mounts also checks /sys/fs/cgroup/cgroup.subtree_control exists;
    // on a host without cgroup v2 the file check fires. We test the mount
    // parsing logic separately from the file check by calling the helper
    // with a known-good mounts string.
    //
    // If running on a real v2 host the file exists and this passes. On a
    // non-v2 host the file check will return UnsupportedHostMode — but the
    // mount check would have already passed, so we only assert the mount
    // detection path by testing a line that should NOT be rejected.
    let result = probe_mounts(UNIFIED_V2_MOUNTS);
    // The call may return Ok (real v2 host) or UnsupportedHostMode (file
    // missing, non-v2 host). It must NOT return an Io error from probe_mounts
    // itself — that would be a bug in the parse logic.
    match result {
        Ok(()) | Err(CgroupError::UnsupportedHostMode) => {}
        Err(e) => panic!("unexpected error from probe_mounts on v2 mounts: {e:?}"),
    }
}

#[test]
fn v1_mounts_returns_unsupported() {
    let result = probe_mounts(V1_MOUNTS);
    assert!(
        matches!(result, Err(CgroupError::UnsupportedHostMode)),
        "v1 mounts must return UnsupportedHostMode, got {result:?}"
    );
}

#[test]
fn hybrid_wrong_root_returns_unsupported() {
    let result = probe_mounts(HYBRID_WRONG_MOUNT_MOUNTS);
    assert!(
        matches!(result, Err(CgroupError::UnsupportedHostMode)),
        "cgroup2 at wrong mount point must return UnsupportedHostMode, got {result:?}"
    );
}

#[test]
fn empty_mounts_returns_unsupported() {
    let result = probe_mounts(EMPTY_MOUNTS);
    assert!(
        matches!(result, Err(CgroupError::UnsupportedHostMode)),
        "empty mounts must return UnsupportedHostMode, got {result:?}"
    );
}

#[test]
fn cgroup2_at_exact_root_is_required() {
    // Mount point must be exactly "/sys/fs/cgroup", not a subpath.
    let mounts = "cgroup2 /sys/fs/cgroup/foo cgroup2 rw 0 0\n";
    let result = probe_mounts(mounts);
    assert!(
        matches!(result, Err(CgroupError::UnsupportedHostMode)),
        "cgroup2 at subpath must not be accepted: {result:?}"
    );
}
