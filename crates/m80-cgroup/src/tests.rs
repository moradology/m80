use super::*;

#[test]
fn required_subtree_control_enables_three_controllers() {
    assert_eq!(Limits::default().required_controllers(), BASE_CONTROLLERS);
    assert_eq!(
        Limits::m80_default().required_controllers(),
        vec!["cpu", "memory", "pids", "io"]
    );
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
fn create_applies_limits_before_pid_enrollment() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("sys/fs/cgroup");
    let parent = base.join("m80-firecracker");
    let leaf = parent.join("vm-ordered");
    fs::create_dir_all(&leaf).unwrap();
    for path in [&base, &parent] {
        fs::write(path.join("cgroup.controllers"), "cpu memory pids io\n").unwrap();
        fs::write(path.join("cgroup.subtree_control"), "").unwrap();
        fs::write(path.join("cpuset.cpus"), "0-1\n").unwrap();
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
    let jailed = JailedFirecracker {
        jailer_pid: 11,
        firecracker_pid: 22,
    };

    let err = Subtree::create_at(&base, &parent, "vm-ordered", dir.path(), &jailed, &limits)
        .expect_err("missing cgroup.procs must fail at enrollment");

    assert!(matches!(err, CgroupError::Io { path, .. } if path == leaf.join("cgroup.procs")));
    assert_eq!(fs::read_to_string(leaf.join("cpu.max")).unwrap(), "max\n");
    assert_eq!(
        fs::read_to_string(leaf.join("memory.max")).unwrap(),
        "1024\n"
    );
    assert_eq!(fs::read_to_string(leaf.join("pids.max")).unwrap(), "9\n");
    assert_eq!(
        fs::read_to_string(leaf.join("io.weight")).unwrap(),
        "default 200\n"
    );
    assert_eq!(
        fs::read_to_string(leaf.join("io.max")).unwrap(),
        "8:0 rbps=1000\n"
    );
    assert_eq!(
        fs::read_to_string(leaf.join("cpuset.cpus")).unwrap(),
        "0-1\n"
    );
    assert_eq!(fs::read_to_string(leaf.join("cpuset.mems")).unwrap(), "0\n");
    assert_subtree_control_contains_all_requested(&base);
    assert_subtree_control_contains_all_requested(&parent);
}

fn assert_subtree_control_contains_all_requested(path: &std::path::Path) {
    let content = fs::read_to_string(path.join("cgroup.subtree_control")).unwrap();
    for controller in ["+cpu", "+memory", "+pids", "+io"] {
        assert!(
            content.split_whitespace().any(|entry| entry == controller),
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

    let jailed = JailedFirecracker {
        jailer_pid: 11,
        firecracker_pid: 22,
    };
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
