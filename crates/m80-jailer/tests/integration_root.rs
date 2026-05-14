//! Integration tests requiring root / CAP_SYS_ADMIN.
//!
//! All tests in this file are marked `#[ignore]` — they require real mount
//! privileges and are not expected to run in CI. Run manually as root:
//!
//! ```sh
//! sudo cargo test -p m80-jailer -- --ignored
//! ```
#![allow(clippy::unwrap_used)]

use m80_jailer::{
    jail_root_path, BindMode, Binding, InspectionDecision, JailerConfig, JailerError, JailerSocket,
    Plan,
};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;
use std::process::Command;

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
        jailer_harden_bin: Some(PathBuf::from("/usr/bin/m80-jailer-harden")),
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
        resource_limits: m80_jailer::ResourceLimits::default(),
        new_pid_ns: false,
        daemonize: false,
        new_cgroup_ns: false,
        netns_path: None,
        stdio_log: None,
    };

    let plan = Plan::compute(&cfg).unwrap();
    let jail = plan
        .materialize()
        .expect("materialize must succeed as root");

    assert!(
        jail.jail_root().exists(),
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
    assert_mount_private(&jail.jail_root().join("kernel/vmlinux"));
}

#[test]
#[ignore = "requires CAP_SYS_ADMIN / root"]
fn materialize_binds_proc_fd_source_without_reopening_original_path() {
    use std::io::Write;
    use std::os::fd::AsRawFd;

    let run_dir = tempfile::tempdir().unwrap();
    let mut source = tempfile::NamedTempFile::new().unwrap();
    source.write_all(b"verified-rootfs").unwrap();
    let held = source.reopen().unwrap();
    let proc_fd = PathBuf::from(format!("/proc/self/fd/{}", held.as_raw_fd()));
    let original_path = source.path().to_path_buf();
    let swapped_path = run_dir.path().join("renamed-verified-rootfs");
    std::fs::rename(&original_path, &swapped_path).unwrap();
    std::fs::write(&original_path, b"attacker-rootfs").unwrap();

    let cfg = JailerConfig {
        jailer_bin: PathBuf::from("/usr/bin/jailer"),
        jailer_harden_bin: Some(PathBuf::from("/usr/bin/m80-jailer-harden")),
        firecracker_bin: PathBuf::from("/usr/bin/firecracker"),
        run_dir: run_dir.path().to_path_buf(),
        uid: 3000,
        gid: 3000,
        bindings: vec![Binding {
            source: proc_fd,
            dest: PathBuf::from("rootfs.ext4"),
            mode: BindMode::Ro,
        }],
        sockets: Vec::new(),
        resource_limits: m80_jailer::ResourceLimits::default(),
        new_pid_ns: false,
        daemonize: false,
        new_cgroup_ns: false,
        netns_path: None,
        stdio_log: None,
    };

    let jail = Plan::compute(&cfg)
        .unwrap()
        .materialize()
        .expect("proc-fd bind must materialize as root");

    let mounted = jail.jail_root().join("rootfs.ext4");
    assert_eq!(
        std::fs::read_to_string(&mounted).unwrap(),
        "verified-rootfs",
        "bind mount must read from held fd, not from the swapped original path"
    );
    assert_mount_private(&mounted);
}

#[test]
#[ignore = "requires CAP_SYS_ADMIN / root"]
fn jailer_placeholder_cleanup_on_partial_bind_failure() {
    let run_dir = tempfile::tempdir().unwrap();
    let first_file = tempfile::NamedTempFile::new().unwrap();
    let second_dir = tempfile::tempdir().unwrap();
    let firecracker_bin = PathBuf::from("/usr/bin/firecracker");
    let cfg = JailerConfig {
        jailer_bin: PathBuf::from("/usr/bin/jailer"),
        jailer_harden_bin: Some(PathBuf::from("/usr/bin/m80-jailer-harden")),
        firecracker_bin: firecracker_bin.clone(),
        run_dir: run_dir.path().to_path_buf(),
        uid: 3000,
        gid: 3000,
        bindings: vec![
            Binding {
                source: first_file.path().to_path_buf(),
                dest: PathBuf::from("first.bin"),
                mode: BindMode::Ro,
            },
            Binding {
                source: second_dir.path().to_path_buf(),
                dest: PathBuf::from("second.bin"),
                mode: BindMode::Ro,
            },
        ],
        sockets: Vec::new(),
        resource_limits: m80_jailer::ResourceLimits::default(),
        new_pid_ns: false,
        daemonize: false,
        new_cgroup_ns: false,
        netns_path: None,
        stdio_log: None,
    };
    let jail_root = jail_root_path(run_dir.path(), &firecracker_bin);
    let first_placeholder = jail_root.join("first.bin");

    let err = Plan::compute(&cfg)
        .unwrap()
        .materialize()
        .expect_err("second bind must fail");

    assert!(
        err.to_string().contains("bind-mount failed"),
        "unexpected materialize error: {err:?}"
    );
    assert!(
        !first_placeholder.exists(),
        "first bind placeholder must be removed after partial materialize failure"
    );
}

#[test]
#[ignore = "requires CAP_SYS_ADMIN / root"]
fn jailer_partial_failure_mid_bind_reverses_prior_steps() {
    let run_dir = tempfile::tempdir().unwrap();
    let file_one = tempfile::NamedTempFile::new().unwrap();
    let file_two = tempfile::NamedTempFile::new().unwrap();
    let file_three = tempfile::NamedTempFile::new().unwrap();
    let failing_dir = tempfile::tempdir().unwrap();
    let firecracker_bin = PathBuf::from("/usr/bin/firecracker");
    let cfg = JailerConfig {
        jailer_bin: PathBuf::from("/usr/bin/jailer"),
        jailer_harden_bin: Some(PathBuf::from("/usr/bin/m80-jailer-harden")),
        firecracker_bin: firecracker_bin.clone(),
        run_dir: run_dir.path().to_path_buf(),
        uid: 3000,
        gid: 3000,
        bindings: vec![
            Binding {
                source: file_one.path().to_path_buf(),
                dest: PathBuf::from("one.bin"),
                mode: BindMode::Ro,
            },
            Binding {
                source: file_two.path().to_path_buf(),
                dest: PathBuf::from("nested/two.bin"),
                mode: BindMode::Ro,
            },
            Binding {
                source: file_three.path().to_path_buf(),
                dest: PathBuf::from("three.bin"),
                mode: BindMode::Ro,
            },
            Binding {
                source: failing_dir.path().to_path_buf(),
                dest: PathBuf::from("four.bin"),
                mode: BindMode::Ro,
            },
        ],
        sockets: Vec::new(),
        resource_limits: m80_jailer::ResourceLimits::default(),
        new_pid_ns: false,
        daemonize: false,
        new_cgroup_ns: false,
        netns_path: None,
        stdio_log: None,
    };
    let jail_root = jail_root_path(run_dir.path(), &firecracker_bin);

    let err = Plan::compute(&cfg)
        .unwrap()
        .materialize()
        .expect_err("fourth bind must fail");

    assert!(
        err.to_string().contains("bind-mount failed"),
        "unexpected materialize error: {err:?}"
    );
    for path in ["one.bin", "nested/two.bin", "three.bin"] {
        assert!(
            !jail_root.join(path).exists(),
            "placeholder or bind residue survived at {path}"
        );
    }
    assert_no_mountinfo_references(run_dir.path());
    assert!(matches!(
        m80_jailer::inspect_run_dir(run_dir.path()).unwrap(),
        InspectionDecision::NoJail
    ));
}

#[test]
#[ignore = "requires CAP_SYS_ADMIN / root"]
fn jailer_pid_timeout_recovery_returns_orphan_jail() {
    let run_dir = tempfile::tempdir().unwrap();
    let jailer_bin = run_dir.path().join("fake-jailer-no-pid.sh");
    std::fs::write(
        &jailer_bin,
        r#"#!/bin/sh
id=
chroot_base=
exec_file=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --id) id="$2"; shift 2 ;;
    --chroot-base-dir) chroot_base="$2"; shift 2 ;;
    --exec-file) exec_file="$2"; shift 2 ;;
    --) shift; break ;;
    *) shift ;;
  esac
done
exec_base="${exec_file##*/}"
jail_root="$chroot_base/$exec_base/$id/root"
/bin/mkdir -p "$jail_root"
/bin/sleep 30
"#,
    )
    .unwrap();
    let mut perms = std::fs::metadata(&jailer_bin).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&jailer_bin, perms).unwrap();

    let cfg = JailerConfig {
        jailer_bin,
        jailer_harden_bin: None,
        firecracker_bin: PathBuf::from("/usr/bin/firecracker"),
        run_dir: run_dir.path().to_path_buf(),
        uid: 3000,
        gid: 3000,
        bindings: Vec::new(),
        sockets: Vec::new(),
        resource_limits: m80_jailer::ResourceLimits::default(),
        new_pid_ns: false,
        daemonize: false,
        new_cgroup_ns: false,
        netns_path: None,
        stdio_log: None,
    };

    let plan = Plan::compute(&cfg).unwrap();
    let jail = plan
        .materialize()
        .expect("materialize must succeed as root");
    let err = jail
        .launch(PathBuf::from("firecracker.sock").as_path())
        .expect_err("missing firecracker.pid must time out");

    assert!(matches!(err, JailerError::FirecrackerPidTimeout { .. }));
    assert!(
        jail.jail_root().exists(),
        "timeout must leave materialized jail residue for recovery/drop"
    );
    let InspectionDecision::OrphanJail { reap_plan } =
        m80_jailer::inspect_run_dir(run_dir.path()).unwrap()
    else {
        panic!("initial state without pids must be recoverable orphan residue");
    };
    assert!(
        !reap_plan.is_empty(),
        "recoverable orphan must carry plan reap plan"
    );
    drop(jail);
    assert_no_mountinfo_references(run_dir.path());
}

#[test]
#[ignore = "requires CAP_SYS_ADMIN / root and setpriv"]
fn jailer_dir_perms_enforced_against_non_owner() {
    let run_dir = tempfile::tempdir().unwrap();
    let mut run_dir_perms = std::fs::metadata(run_dir.path()).unwrap().permissions();
    run_dir_perms.set_mode(0o755);
    std::fs::set_permissions(run_dir.path(), run_dir_perms).unwrap();

    let cfg = JailerConfig {
        jailer_bin: PathBuf::from("/usr/bin/jailer"),
        jailer_harden_bin: Some(PathBuf::from("/usr/bin/m80-jailer-harden")),
        firecracker_bin: PathBuf::from("/usr/bin/firecracker"),
        run_dir: run_dir.path().to_path_buf(),
        uid: 3000,
        gid: 3000,
        bindings: Vec::new(),
        sockets: Vec::new(),
        resource_limits: m80_jailer::ResourceLimits::default(),
        new_pid_ns: false,
        daemonize: false,
        new_cgroup_ns: false,
        netns_path: None,
        stdio_log: None,
    };

    let jail = Plan::compute(&cfg)
        .unwrap()
        .materialize()
        .expect("materialize must succeed as root");
    let meta = std::fs::metadata(jail.jail_root()).expect("jail root metadata");
    assert_eq!(meta.mode() & 0o777, 0o700);
    assert_eq!(meta.uid(), 3000);
    assert_eq!(meta.gid(), 3000);

    let output = Command::new("setpriv")
        .args([
            "--reuid",
            "3001",
            "--regid",
            "3001",
            "--clear-groups",
            "--",
            "/bin/ls",
        ])
        .arg(jail.jail_root())
        .output()
        .expect("setpriv must run");
    assert!(
        !output.status.success(),
        "non-owner unexpectedly listed jail root: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Permission denied"),
        "non-owner failure should be EACCES: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "requires CAP_SYS_ADMIN / root and real Firecracker jailer"]
fn launch_with_new_pid_ns_records_sentinel_and_firecracker_is_pid_one() {
    let run_root = PathBuf::from(
        std::env::var("M80_RUN_ROOT").unwrap_or_else(|_| "/var/lib/m80-run".to_owned()),
    );
    std::fs::create_dir_all(&run_root).expect("run root");
    let run_dir = tempfile::Builder::new()
        .prefix("m80-jailer-newpid-")
        .tempdir_in(&run_root)
        .expect("temp run dir");

    let cfg = JailerConfig {
        jailer_bin: std::env::var("M80_JAILER_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/opt/firecracker/bin/jailer")),
        jailer_harden_bin: Some(
            std::env::var("M80_JAILER_HARDEN_BIN")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/opt/m80/bin/m80-jailer-harden")),
        ),
        firecracker_bin: std::env::var("M80_FIRECRACKER_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/opt/firecracker/bin/firecracker")),
        run_dir: run_dir.path().to_path_buf(),
        uid: 3000,
        gid: 3000,
        bindings: Vec::new(),
        sockets: vec![JailerSocket::Firecracker],
        resource_limits: m80_jailer::ResourceLimits {
            no_file: 77,
            fsize: None,
            nproc: Some(128),
            memlock: Some(0),
            address_space: None,
            core: Some(0),
            stack: Some(8 * 1024 * 1024),
        },
        new_pid_ns: true,
        daemonize: false,
        new_cgroup_ns: false,
        netns_path: None,
        stdio_log: Some(run_dir.path().join("console.log")),
    };

    let plan = Plan::compute(&cfg).unwrap();
    let jail = plan
        .materialize()
        .expect("materialize must succeed as root");
    let jailed = jail
        .launch(PathBuf::from("firecracker.sock").as_path())
        .unwrap();

    assert_eq!(jailed.jailer_pid(), 0);
    let state: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(run_dir.path().join("jailer-state.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(state["jailer_pid"], 0);
    assert_eq!(state["firecracker_pid"], jailed.firecracker_pid());

    let status =
        std::fs::read_to_string(format!("/proc/{}/status", jailed.firecracker_pid())).unwrap();
    let nspid = status
        .lines()
        .find(|line| line.starts_with("NSpid:"))
        .unwrap_or_else(|| panic!("missing NSpid in status:\n{status}"));
    assert!(
        nspid.split_whitespace().last() == Some("1"),
        "firecracker must be PID 1 in its new PID namespace: {nspid}"
    );
    assert_limit_contains(jailed.firecracker_pid(), "Max open files", "77", "77");
    assert_status_contains(jailed.firecracker_pid(), "Uid:", "3000\t3000\t3000\t3000");
    assert_status_contains(jailed.firecracker_pid(), "Gid:", "3000\t3000\t3000\t3000");
    assert_status_contains(jailed.firecracker_pid(), "NoNewPrivs:", "1");
    assert_status_contains(jailed.firecracker_pid(), "CapPrm:", "0000000000000000");
    assert_status_contains(jailed.firecracker_pid(), "CapEff:", "0000000000000000");
    assert_status_contains(jailed.firecracker_pid(), "CapInh:", "0000000000000000");
    assert_status_contains(jailed.firecracker_pid(), "CapAmb:", "0000000000000000");
    assert_status_contains(jailed.firecracker_pid(), "CapBnd:", "0000000000000000");
    assert_status_contains(jailed.firecracker_pid(), "SigBlk:", "0000000000000000");
    assert_supplementary_groups_empty(jailed.firecracker_pid());
    assert_exec_file_is_private_copy(
        jailed.firecracker_pid(),
        &cfg.firecracker_bin,
        cfg.uid,
        cfg.gid,
    );

    kill(
        Pid::from_raw(jailed.firecracker_pid() as i32),
        Signal::SIGKILL,
    )
    .expect("kill firecracker");
}

fn assert_exec_file_is_private_copy(pid: u32, source: &std::path::Path, uid: u32, gid: u32) {
    let copied = PathBuf::from(format!("/proc/{pid}/root/firecracker"));
    let source_meta = std::fs::metadata(source).expect("source firecracker metadata");
    let copied_meta = std::fs::metadata(&copied).expect("copied firecracker metadata");

    assert_ne!(
        (source_meta.dev(), source_meta.ino()),
        (copied_meta.dev(), copied_meta.ino()),
        "jailed firecracker binary must be a copy, not a bind mount or hard link"
    );
    assert_eq!(
        copied_meta.nlink(),
        1,
        "copied binary must not be hard-linked"
    );
    assert_eq!(copied_meta.uid(), uid, "copied binary uid");
    assert_eq!(copied_meta.gid(), gid, "copied binary gid");
    assert_eq!(
        copied_meta.mode() & 0o777,
        0o700,
        "copied binary mode must be owner-only under m80 hardening"
    );
}

fn assert_status_contains(pid: u32, label: &str, expected: &str) {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).expect("status");
    let line = status
        .lines()
        .find(|line| line.starts_with(label))
        .unwrap_or_else(|| panic!("missing {label} in status:\n{status}"));
    assert!(
        line.contains(expected),
        "{label} line does not contain {expected:?}: {line}"
    );
}

fn assert_supplementary_groups_empty(pid: u32) {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).expect("status");
    let line = status
        .lines()
        .find(|line| line.starts_with("Groups:"))
        .unwrap_or_else(|| panic!("missing Groups in status:\n{status}"));
    assert_eq!(line.trim(), "Groups:", "supplementary groups must be empty");
}

fn assert_limit_contains(pid: u32, label: &str, soft: &str, hard: &str) {
    let limits = std::fs::read_to_string(format!("/proc/{pid}/limits")).expect("limits");
    let line = limits
        .lines()
        .find(|line| line.starts_with(label))
        .unwrap_or_else(|| panic!("missing {label} in limits:\n{limits}"));
    assert!(
        line.contains(soft) && line.contains(hard),
        "{label} line does not contain expected soft/hard limits {soft}/{hard}: {line}"
    );
}

fn assert_no_mountinfo_references(path: &std::path::Path) {
    let mountinfo = std::fs::read_to_string("/proc/self/mountinfo").expect("host mountinfo");
    let needle = path.to_string_lossy();
    assert!(
        !mountinfo.contains(needle.as_ref()),
        "host mountinfo still references {} after materialize failure:\n{mountinfo}",
        path.display()
    );
}

fn assert_mount_private(path: &std::path::Path) {
    let mountinfo = std::fs::read_to_string("/proc/self/mountinfo").expect("host mountinfo");
    let needle = path.to_string_lossy();
    let line = mountinfo
        .lines()
        .find(|line| line.split_whitespace().nth(4) == Some(needle.as_ref()))
        .unwrap_or_else(|| {
            panic!(
                "missing mountinfo entry for {}:\n{mountinfo}",
                path.display()
            )
        });
    let optional_fields = line
        .split(" - ")
        .next()
        .expect("mountinfo separator present before fs fields");
    assert!(
        !optional_fields
            .split_whitespace()
            .any(|field| field.starts_with("shared:") || field.starts_with("master:")),
        "{} must not be shared or slave propagated: {line}",
        path.display()
    );
}
