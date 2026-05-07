//! Integration tests requiring root / CAP_SYS_ADMIN.
//!
//! All tests in this file are marked `#[ignore]` — they require real mount
//! privileges and are not expected to run in CI. Run manually as root:
//!
//! ```sh
//! sudo cargo test -p m80-jailer -- --ignored
//! ```

use m80_jailer::{BindMode, Binding, JailerConfig, JailerSocket, Plan};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use std::os::unix::fs::MetadataExt;
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

    assert_eq!(jailed.jailer_pid, 0);
    let state: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(run_dir.path().join("jailer-state.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(state["jailer_pid"], 0);
    assert_eq!(state["firecracker_pid"], jailed.firecracker_pid);

    let status =
        std::fs::read_to_string(format!("/proc/{}/status", jailed.firecracker_pid)).unwrap();
    let nspid = status
        .lines()
        .find(|line| line.starts_with("NSpid:"))
        .unwrap_or_else(|| panic!("missing NSpid in status:\n{status}"));
    assert!(
        nspid.split_whitespace().last() == Some("1"),
        "firecracker must be PID 1 in its new PID namespace: {nspid}"
    );
    assert_limit_contains(jailed.firecracker_pid, "Max open files", "77", "77");
    assert_status_contains(jailed.firecracker_pid, "Uid:", "3000\t3000\t3000\t3000");
    assert_status_contains(jailed.firecracker_pid, "Gid:", "3000\t3000\t3000\t3000");
    assert_status_contains(jailed.firecracker_pid, "NoNewPrivs:", "1");
    assert_status_contains(jailed.firecracker_pid, "CapPrm:", "0000000000000000");
    assert_status_contains(jailed.firecracker_pid, "CapEff:", "0000000000000000");
    assert_status_contains(jailed.firecracker_pid, "CapInh:", "0000000000000000");
    assert_status_contains(jailed.firecracker_pid, "CapAmb:", "0000000000000000");
    assert_status_contains(jailed.firecracker_pid, "SigBlk:", "0000000000000000");
    assert_supplementary_groups_empty(jailed.firecracker_pid);
    assert_exec_file_is_private_copy(
        jailed.firecracker_pid,
        &cfg.firecracker_bin,
        cfg.uid,
        cfg.gid,
    );

    kill(
        Pid::from_raw(jailed.firecracker_pid as i32),
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
