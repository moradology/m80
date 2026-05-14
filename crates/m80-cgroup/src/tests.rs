use super::*;

use std::env;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;

const REAL_CGROUP_PARENT: &str = "/sys/fs/cgroup/m80-firecracker";
const UNIFIED_V2_MOUNTS: &str = "\
sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0
cgroup2 /sys/fs/cgroup cgroup2 rw,nosuid,nodev,noexec,relatime,nsdelegate,memory_recursiveprot 0 0
";
const V1_MOUNTS: &str = "\
sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0
cgroup /sys/fs/cgroup/cpu,cpuacct cgroup rw,cpu,cpuacct 0 0
cgroup /sys/fs/cgroup/memory cgroup rw,memory 0 0
cgroup /sys/fs/cgroup/pids cgroup rw,pids 0 0
";
const HYBRID_WRONG_MOUNT_MOUNTS: &str = "\
sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0
cgroup2 /sys/fs/cgroup/unified cgroup2 rw 0 0
cgroup /sys/fs/cgroup/memory cgroup rw,memory 0 0
";

#[test]
fn required_subtree_control_enables_three_controllers() {
    assert_eq!(Limits::default().required_controllers(), BASE_CONTROLLERS);
    // preset() does not set io_weight, so io controller is not required by default
    assert_eq!(
        Limits::preset().required_controllers(),
        vec!["cpu", "memory", "pids"]
    );
    let pinned = Limits {
        cpuset_cpus: Some("0".to_owned()),
        ..Limits::default()
    };
    assert_eq!(
        pinned.required_controllers(),
        vec!["cpu", "memory", "pids", "cpuset"]
    );
}

#[test]
fn unified_v2_mounts_accepts() {
    let result = probe::probe_mounts(UNIFIED_V2_MOUNTS);
    assert!(
        result.is_ok(),
        "probe_mounts must accept a well-formed unified v2 mount table: {result:?}"
    );
}

#[test]
fn v1_mounts_returns_unsupported() {
    let result = probe::probe_mounts(V1_MOUNTS);
    assert!(
        matches!(result, Err(CgroupError::UnsupportedHostMode)),
        "v1 mounts must return UnsupportedHostMode, got {result:?}"
    );
}

#[test]
fn hybrid_wrong_root_returns_unsupported() {
    let result = probe::probe_mounts(HYBRID_WRONG_MOUNT_MOUNTS);
    assert!(
        matches!(result, Err(CgroupError::UnsupportedHostMode)),
        "cgroup2 at wrong mount point must return UnsupportedHostMode, got {result:?}"
    );
}

#[test]
fn empty_mounts_returns_unsupported() {
    let result = probe::probe_mounts("");
    assert!(
        matches!(result, Err(CgroupError::UnsupportedHostMode)),
        "empty mounts must return UnsupportedHostMode, got {result:?}"
    );
}

#[test]
fn cgroup2_at_exact_root_is_required() {
    let mounts = "cgroup2 /sys/fs/cgroup/foo cgroup2 rw 0 0\n";
    let result = probe::probe_mounts(mounts);
    assert!(
        matches!(result, Err(CgroupError::UnsupportedHostMode)),
        "cgroup2 at subpath must not be accepted: {result:?}"
    );
}

#[test]
fn probe_cache_reuses_successful_result() {
    let cache = OnceLock::new();
    let calls = AtomicUsize::new(0);

    probe::probe_with_cache(&cache, || {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(UNIFIED_V2_MOUNTS.to_string())
    })
    .expect("first probe must accept unified v2");

    probe::probe_with_cache(&cache, || {
        calls.fetch_add(1, Ordering::SeqCst);
        Err(CgroupError::UnsupportedHostMode)
    })
    .expect("second probe must replay the cached success");

    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn probe_cache_replays_first_error() {
    let cache = OnceLock::new();
    let calls = AtomicUsize::new(0);
    let path = PathBuf::from("/proc/mounts");

    let first = probe::probe_with_cache(&cache, || {
        calls.fetch_add(1, Ordering::SeqCst);
        Err(CgroupError::Io {
            path: path.clone(),
            source: io::Error::new(io::ErrorKind::PermissionDenied, "synthetic denial"),
        })
    });
    assert!(matches!(first, Err(CgroupError::Io { path: p, .. }) if p == path));

    let second = probe::probe_with_cache(&cache, || {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(UNIFIED_V2_MOUNTS.to_string())
    });
    assert!(matches!(second, Err(CgroupError::Io { path: p, .. }) if p == path));

    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn public_probe_reads_host_once_from_fresh_process() {
    if env::var_os("M80_CGROUP_PROBE_CHILD").is_some() {
        return;
    }

    let status = Command::new(env::current_exe().expect("current test binary"))
        .env("M80_CGROUP_PROBE_CHILD", "1")
        .arg("--exact")
        .arg("tests::public_probe_child_reads_once")
        .arg("--nocapture")
        .status()
        .expect("spawn public probe child test");

    assert_eq!(
        status.code(),
        Some(0),
        "child public probe cache test must pass"
    );
}

#[test]
fn public_probe_child_reads_once() {
    if env::var_os("M80_CGROUP_PROBE_CHILD").is_none() {
        return;
    }

    assert_eq!(probe::host_probe_read_count(), 0);
    let first = Subtree::probe();
    assert_eq!(probe::host_probe_read_count(), 1);
    let second = Subtree::probe();
    assert_eq!(probe::host_probe_read_count(), 1);
    assert_eq!(probe_result_kind(&first), probe_result_kind(&second));
}

fn probe_result_kind(result: &Result<(), CgroupError>) -> &'static str {
    match result {
        Ok(()) => "ok",
        Err(CgroupError::UnsupportedHostMode) => "unsupported",
        Err(CgroupError::ControllerNotEnabled(_)) => "controller",
        Err(CgroupError::SparseInheritedFile(_)) => "cpuset",
        Err(CgroupError::InvalidLimit { .. }) => "limit",
        Err(CgroupError::LivePids { .. }) => "live-pids",
        Err(CgroupError::Io { .. }) => "io",
    }
}

#[test]
fn pid_assignment_sorts_and_deduplicates() {
    assert_eq!(enrolled_pids(20, 10), vec![10, 20]);
}

#[test]
fn pid_assignment_collapses_exec_equal_pids() {
    assert_eq!(enrolled_pids(10, 10), vec![10]);
}

#[test]
fn pid_assignment_skips_new_pid_namespace_sentinel() {
    assert_eq!(enrolled_pids(0, 10), vec![10]);
}

#[test]
fn io_max_formats_v2_row() {
    assert_eq!(
        IoMax {
            major: 8,
            minor: 0,
            rbps: Some(1024),
            wbps: None,
            riops: Some(10),
            wiops: None,
        }
        .to_string(),
        "8:0 rbps=1024 riops=10"
    );
}

#[test]
fn io_weight_range_is_kernel_bounded() {
    validate_io_weight(1).unwrap();
    validate_io_weight(10_000).unwrap();
    assert!(matches!(
        validate_io_weight(0),
        Err(CgroupError::InvalidLimit {
            field: "io_weight",
            ..
        })
    ));
}

#[test]
fn cpuset_cpus_rejects_empty_or_spaced_values() {
    for value in ["", " ", "0 1", " 0", "0\n"] {
        assert!(matches!(
            validate_cpuset_cpus(value),
            Err(CgroupError::InvalidLimit {
                field: "cpuset_cpus",
                ..
            })
        ));
    }
    validate_cpuset_cpus("0-1,4").unwrap();
}

#[test]
fn create_applies_limits_before_pid_enrollment() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("sys/fs/cgroup");
    let parent = base.join("m80-firecracker");
    let leaf = parent.join("vm-ordered");
    fs::create_dir_all(&leaf).unwrap();
    for path in [&base, &parent] {
        fs::write(
            path.join("cgroup.controllers"),
            "cpu memory pids cpuset io\n",
        )
        .unwrap();
        fs::write(path.join("cgroup.subtree_control"), "").unwrap();
        fs::write(path.join("cpuset.cpus"), "0\n").unwrap();
        fs::write(path.join("cpuset.mems"), "0\n").unwrap();
    }
    for name in [
        "cpu.max",
        "memory.max",
        "pids.max",
        "io.weight",
        "io.max",
        "cpuset.cpus",
        "cpuset.mems",
    ] {
        fs::write(leaf.join(name), "").unwrap();
    }

    let limits = Limits {
        cpu_max: Some(CpuMax::Max),
        memory_max: Some(1024),
        pids_max: Some(9),
        cpuset_cpus: Some("0".to_owned()),
        io_max: vec![IoMax {
            major: 8,
            minor: 0,
            rbps: Some(1000),
            wbps: None,
            riops: None,
            wiops: None,
        }],
        io_weight: Some(200),
        oom_score_adj: None,
    };
    let jailed = JailedFirecracker::new(11, 22);

    let err = Subtree::create_at(&base, &parent, "vm-ordered", dir.path(), &jailed, &limits)
        .expect_err("missing cgroup.procs must fail at enrollment");

    assert!(matches!(err, CgroupError::Io { path, .. } if path == leaf.join("cgroup.procs")));
    assert_eq!(fs::read_to_string(leaf.join("cpu.max")).unwrap(), "max\n");
    assert_eq!(
        fs::read_to_string(leaf.join("memory.max")).unwrap(),
        "1024\n"
    );
    assert_eq!(fs::read_to_string(leaf.join("pids.max")).unwrap(), "9\n");
    assert_eq!(fs::read_to_string(leaf.join("cpuset.cpus")).unwrap(), "0\n");
    assert_eq!(
        fs::read_to_string(leaf.join("io.weight")).unwrap(),
        "default 200\n"
    );
    assert_eq!(
        fs::read_to_string(leaf.join("io.max")).unwrap(),
        "8:0 rbps=1000\n"
    );
    assert_eq!(fs::read_to_string(leaf.join("cpuset.mems")).unwrap(), "0\n");
    assert_subtree_control_contains_all_requested(&base);
    assert_subtree_control_contains_all_requested(&parent);
}

#[test]
fn explicit_cpuset_cpus_skips_sparse_cpu_inheritance_and_uses_effective_mems() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("sys/fs/cgroup");
    let parent = base.join("m80-firecracker");
    let leaf = parent.join("vm-explicit-cpuset");
    fs::create_dir_all(&leaf).unwrap();
    for path in [&base, &parent] {
        fs::write(path.join("cgroup.controllers"), "cpu memory pids cpuset\n").unwrap();
        fs::write(path.join("cgroup.subtree_control"), "").unwrap();
        fs::write(path.join("cpuset.cpus"), "\n").unwrap();
        fs::write(path.join("cpuset.cpus.effective"), "0-3\n").unwrap();
        fs::write(path.join("cpuset.mems"), "\n").unwrap();
        fs::write(path.join("cpuset.mems.effective"), "0\n").unwrap();
    }
    fs::write(leaf.join("cpuset.cpus"), "").unwrap();
    fs::write(leaf.join("cpuset.mems"), "").unwrap();

    let limits = Limits {
        cpuset_cpus: Some("1".to_owned()),
        ..Limits::default()
    };
    let jailed = JailedFirecracker::new(11, 22);

    let err = Subtree::create_at(
        &base,
        &parent,
        "vm-explicit-cpuset",
        dir.path(),
        &jailed,
        &limits,
    )
    .expect_err("missing cgroup.procs must fail at enrollment");

    assert!(matches!(err, CgroupError::Io { path, .. } if path == leaf.join("cgroup.procs")));
    assert_eq!(fs::read_to_string(leaf.join("cpuset.cpus")).unwrap(), "1\n");
    assert_eq!(fs::read_to_string(leaf.join("cpuset.mems")).unwrap(), "0\n");
}

#[test]
fn subtree_control_chain_checks_only_root_controller_availability() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("sys/fs/cgroup");
    let parent = base.join("m80-firecracker");
    fs::create_dir_all(&parent).unwrap();
    fs::write(base.join("cgroup.controllers"), "cpu memory pids\n").unwrap();
    fs::write(base.join("cgroup.subtree_control"), "").unwrap();
    fs::write(parent.join("cgroup.subtree_control"), "").unwrap();

    enable_subtree_control_chain(&base, &parent, &["cpu", "memory", "pids"]).unwrap();

    assert_subtree_control_contains(&base, &["+cpu", "+memory", "+pids"]);
    assert_subtree_control_contains(&parent, &["+cpu", "+memory", "+pids"]);
}

#[test]
fn subtree_control_chain_still_rejects_missing_root_controller() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("sys/fs/cgroup");
    let parent = base.join("m80-firecracker");
    fs::create_dir_all(&parent).unwrap();
    fs::write(base.join("cgroup.controllers"), "cpu pids\n").unwrap();
    fs::write(base.join("cgroup.subtree_control"), "").unwrap();
    fs::write(parent.join("cgroup.subtree_control"), "").unwrap();

    let err = enable_subtree_control_chain(&base, &parent, &["cpu", "memory", "pids"])
        .expect_err("missing root memory controller must fail typed");

    assert!(matches!(err, CgroupError::ControllerNotEnabled("memory")));
}

#[test]
fn subtree_control_once_gate_runs_initializer_once_across_threads() {
    let primed = OnceLock::new();
    let calls = AtomicUsize::new(0);

    thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                enable_subtree_control_chain_once(&primed, || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    thread::sleep(Duration::from_millis(5));
                    Ok(())
                })
                .unwrap();
            });
        }
    });

    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn subtree_control_once_gate_does_not_cache_failure() {
    let primed = OnceLock::new();
    let calls = AtomicUsize::new(0);

    let first = enable_subtree_control_chain_once(&primed, || {
        calls.fetch_add(1, Ordering::SeqCst);
        Err(CgroupError::ControllerNotEnabled("cpu"))
    });
    assert!(matches!(
        first,
        Err(CgroupError::ControllerNotEnabled("cpu"))
    ));

    enable_subtree_control_chain_once(&primed, || {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
    .unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

fn assert_subtree_control_contains_all_requested(path: &std::path::Path) {
    assert_subtree_control_contains(path, &["+cpu", "+memory", "+pids", "+cpuset", "+io"]);
}

fn assert_subtree_control_contains(path: &std::path::Path, controllers: &[&str]) {
    let content = fs::read_to_string(path.join("cgroup.subtree_control")).unwrap();
    for controller in controllers {
        assert!(
            content.split_whitespace().any(|entry| entry == *controller),
            "{} missing {controller}: {content:?}",
            path.display()
        );
    }
}

#[test]
fn sparse_cpuset_without_non_empty_ancestor_fails_typed() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("sys/fs/cgroup");
    let parent = base.join("m80-firecracker");
    let leaf = parent.join("vm-sparse");
    fs::create_dir_all(&leaf).unwrap();
    for path in [&base, &parent] {
        fs::write(path.join("cgroup.controllers"), "cpu memory pids\n").unwrap();
        fs::write(path.join("cgroup.subtree_control"), "").unwrap();
        fs::write(path.join("cpuset.cpus"), "\n").unwrap();
    }
    fs::write(leaf.join("cpuset.cpus"), "").unwrap();

    let jailed = JailedFirecracker::new(11, 22);
    let err = Subtree::create_at(
        &base,
        &parent,
        "vm-sparse",
        dir.path(),
        &jailed,
        &Limits::default(),
    )
    .expect_err("empty inherited cpuset must fail typed");

    assert!(matches!(
        err,
        CgroupError::SparseInheritedFile("cpuset.cpus")
    ));
}

#[test]
fn oom_score_adj_range_is_kernel_bounded() {
    assert!(matches!(
        set_oom_score_adj(1, 1001),
        Err(CgroupError::InvalidLimit {
            field: "oom_score_adj",
            ..
        })
    ));
}

#[test]
fn kill_cgroup_writes_kernel_kill_file_when_present() {
    let dir = tempfile::tempdir().unwrap();
    let leaf = dir.path().join("leaf");
    fs::create_dir(&leaf).unwrap();
    fs::write(leaf.join("cgroup.kill"), "").unwrap();

    kill_cgroup(&leaf).unwrap();

    assert_eq!(fs::read_to_string(leaf.join("cgroup.kill")).unwrap(), "1\n");
}

#[test]
fn drop_without_cgroup_kill_removes_empty_temp_leaf() {
    let dir = tempfile::tempdir().unwrap();
    let leaf = dir.path().join("leaf");
    fs::create_dir(&leaf).unwrap();

    drop(Subtree(leaf.clone()));

    assert!(!leaf.exists(), "empty temp leaf must be removed on Drop");
}

#[test]
#[ignore = "requires root and a writable cgroup v2 hierarchy"]
fn cgroup_drop_with_live_procs_uses_cgroup_kill_then_rmdir() {
    Subtree::probe().expect("probe() must return Ok on a unified-v2 host");

    let vm_id = format!("m80-drop-live-{}", std::process::id());
    let leaf = PathBuf::from(REAL_CGROUP_PARENT).join(vm_id);
    if leaf.exists() {
        kill_pids_in_real_cgroup(&leaf);
        wait_for_empty_real_cgroup(&leaf);
        fs::remove_dir(&leaf).expect("remove stale live-proc cgroup leaf");
    }
    fs::create_dir_all(REAL_CGROUP_PARENT).expect("create m80 cgroup parent");
    fs::create_dir(&leaf).expect("create live-proc cgroup leaf");

    let child = Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("spawn cgroup resident");
    write_cgroup_file(&leaf.join("cgroup.procs"), &format!("{}\n", child.id())).unwrap();

    let mut guard = LiveProcCgroupGuard {
        leaf: leaf.clone(),
        child: Some(child),
        cleaned: false,
    };

    drop(Subtree(leaf.clone()));

    assert!(
        !leaf.exists(),
        "Subtree::Drop must kill live procs and remove the cgroup leaf"
    );

    guard.cleanup();
    assert!(
        !leaf.exists(),
        "cleanup guard must leave killed cgroup leaf absent"
    );
}

struct LiveProcCgroupGuard {
    leaf: PathBuf,
    child: Option<Child>,
    cleaned: bool,
}

impl LiveProcCgroupGuard {
    fn cleanup(&mut self) {
        if self.cleaned {
            return;
        }
        kill_pids_in_real_cgroup(&self.leaf);
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        wait_for_empty_real_cgroup(&self.leaf);
        let _ = fs::remove_dir(&self.leaf);
        self.cleaned = true;
    }
}

impl Drop for LiveProcCgroupGuard {
    fn drop(&mut self) {
        self.cleanup();
    }
}

fn kill_pids_in_real_cgroup(leaf: &Path) {
    for pid in read_real_cgroup_pids(leaf) {
        match kill(Pid::from_raw(pid), Signal::SIGKILL) {
            Ok(()) | Err(Errno::ESRCH) => {}
            Err(err) => panic!("SIGKILL {pid}: {err}"),
        }
    }
}

fn wait_for_empty_real_cgroup(leaf: &Path) {
    let started = Instant::now();
    while !read_real_cgroup_pids(leaf).is_empty() {
        assert!(
            started.elapsed() <= Duration::from_secs(5),
            "{} still has live pids: {:?}",
            leaf.display(),
            read_real_cgroup_pids(leaf)
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn read_real_cgroup_pids(leaf: &Path) -> Vec<i32> {
    match fs::read_to_string(leaf.join("cgroup.procs")) {
        Ok(content) => content
            .split_whitespace()
            .map(|pid| pid.parse::<i32>().expect("numeric cgroup pid"))
            .collect(),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(err) => panic!("read {}/cgroup.procs: {err}", leaf.display()),
    }
}
