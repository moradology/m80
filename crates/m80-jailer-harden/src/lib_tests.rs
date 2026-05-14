use super::*;

#[test]
fn parse_requires_separator() {
    let err = parse_args([
        "--jailer-bin",
        "/bin/echo",
        "--uid",
        "3000",
        "--gid",
        "3000",
    ])
    .unwrap_err();
    assert!(matches!(err, HardenError::MissingSeparator));
}

#[test]
fn parse_requires_jailer_args_after_separator() {
    let err = parse_args([
        "--jailer-bin",
        "/bin/echo",
        "--uid",
        "3000",
        "--gid",
        "3000",
        "--",
    ])
    .unwrap_err();
    assert!(matches!(err, HardenError::MissingJailerArgs));
}

#[test]
fn parse_captures_official_jailer_and_forwarded_args() {
    let parsed = parse_args([
        "--jailer-bin",
        "/opt/firecracker/bin/jailer",
        "--uid",
        "3000",
        "--gid",
        "3001",
        "--rlimit",
        "nproc=64",
        "--rlimit",
        "memlock=0",
        "--new-cgroup-ns",
        "--new-net-ns",
        "--",
        "--uid",
        "3000",
        "--gid",
        "3001",
        "--id",
        "vm-1",
    ])
    .unwrap();

    assert_eq!(
        parsed.jailer_bin,
        PathBuf::from("/opt/firecracker/bin/jailer")
    );
    assert_eq!(
        parsed.jailer_args,
        vec!["--uid", "3000", "--gid", "3001", "--id", "vm-1"]
    );
    assert_eq!(
        parsed.resource_limits(),
        &[
            ResourceLimit {
                kind: ResourceLimitKind::NProc,
                value: 64,
            },
            ResourceLimit {
                kind: ResourceLimitKind::MemLock,
                value: 0,
            },
        ]
    );
    assert_eq!(
        parsed.resource_limits,
        vec![
            ResourceLimit {
                kind: ResourceLimitKind::NProc,
                value: 64,
            },
            ResourceLimit {
                kind: ResourceLimitKind::MemLock,
                value: 0,
            },
        ]
    );
    assert!(parsed.new_cgroup_ns());
    assert!(parsed.new_net_ns());
}

#[test]
fn parse_rejects_bad_uid() {
    let err = parse_args([
        "--jailer-bin",
        "/bin/echo",
        "--uid",
        "not-a-uid",
        "--gid",
        "3000",
        "--",
        "--id",
        "vm-1",
    ])
    .unwrap_err();

    assert!(matches!(
        err,
        HardenError::InvalidValue { field: "--uid", .. }
    ));
}

#[test]
fn parse_requires_uid_before_separator() {
    let err = parse_args([
        "--jailer-bin",
        "/bin/echo",
        "--gid",
        "3000",
        "--",
        "--id",
        "vm",
    ])
    .unwrap_err();
    assert!(matches!(err, HardenError::MissingArgument("--uid")));
}

#[test]
fn parse_requires_gid_before_separator() {
    let err = parse_args([
        "--jailer-bin",
        "/bin/echo",
        "--uid",
        "3000",
        "--",
        "--id",
        "vm",
    ])
    .unwrap_err();
    assert!(matches!(err, HardenError::MissingArgument("--gid")));
}

#[test]
fn parse_rejects_unknown_resource_limit() {
    let err = parse_args([
        "--jailer-bin",
        "/bin/echo",
        "--uid",
        "3000",
        "--gid",
        "3000",
        "--rlimit",
        "unknown=7",
        "--",
        "--id",
        "vm-1",
    ])
    .unwrap_err();

    assert!(matches!(
        err,
        HardenError::InvalidValue {
            field: "--rlimit",
            ..
        }
    ));
}
