#![allow(clippy::unwrap_used)]

use super::*;
use crate::types::{CgroupVersion, JailerConfig, JailerSocket, Plan};
use std::io::Cursor;
use std::os::unix::fs::PermissionsExt;

struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            std::env::set_var(self.key, previous);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

fn wait_for_log_contains(path: &Path, needles: &[&str]) -> String {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let log = std::fs::read_to_string(path).unwrap_or_default();
        if needles.iter().all(|needle| log.contains(needle)) {
            return log;
        }
        if Instant::now() >= deadline {
            panic!("timed out waiting for {needles:?} in {}", path.display());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn firecracker_pid_poll_backoff_starts_at_one_ms_and_caps_at_twenty_five_ms() {
    let mut delay = FIRECRACKER_PID_INITIAL_POLL;
    let mut delays = Vec::new();
    for _ in 0..8 {
        delays.push(delay);
        delay = next_firecracker_pid_poll_delay(delay);
    }

    assert_eq!(
        delays,
        vec![
            Duration::from_millis(1),
            Duration::from_millis(2),
            Duration::from_millis(4),
            Duration::from_millis(8),
            Duration::from_millis(16),
            Duration::from_millis(25),
            Duration::from_millis(25),
            Duration::from_millis(25),
        ]
    );
}

#[test]
fn firecracker_pid_wait_rechecks_after_one_ms_poll() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("firecracker.pid");
    let mut sleeps = Vec::new();

    let pid = wait_for_firecracker_pid_file_with_sleep(
        &pid_file,
        Instant::now() + Duration::from_secs(1),
        |delay| {
            sleeps.push(delay);
            std::fs::write(&pid_file, b"4242\n").unwrap();
        },
    )
    .unwrap()
    .unwrap();

    assert_eq!(pid, 4242);
    assert_eq!(sleeps, vec![Duration::from_millis(1)]);
}

#[test]
fn firecracker_pid_wait_retries_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("firecracker.pid");
    std::fs::write(&pid_file, b"").unwrap();
    let mut sleeps = Vec::new();

    let pid = wait_for_firecracker_pid_file_with_sleep(
        &pid_file,
        Instant::now() + Duration::from_secs(1),
        |delay| {
            sleeps.push(delay);
            std::fs::write(&pid_file, b"4242\n").unwrap();
        },
    )
    .unwrap()
    .unwrap();

    assert_eq!(pid, 4242);
    assert_eq!(sleeps, vec![Duration::from_millis(1)]);
}

#[test]
fn firecracker_pid_wait_rejects_non_numeric_file() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("firecracker.pid");
    std::fs::write(&pid_file, b"not-a-pid\n").unwrap();

    let err = wait_for_firecracker_pid_file_with_sleep(
        &pid_file,
        Instant::now() + Duration::from_secs(1),
        |_| panic!("invalid pid must fail without retrying"),
    )
    .expect_err("non-numeric pid file must fail closed");

    assert!(
        err.to_string().contains("firecracker.pid not a u32"),
        "unexpected error: {err:?}"
    );
}

fn launch_and_record_args(cgroup_version: Option<CgroupVersion>) -> String {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = dir.path().join("vm-cgroup-version");
    std::fs::create_dir_all(&run_dir).unwrap();
    let jailer_bin = dir.path().join("fake-jailer-cgroup-version.sh");
    std::fs::write(
        &jailer_bin,
        r#"#!/bin/sh
id=
chroot_base=
exec_file=
all_args="$*"
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
printf '%s\n' "$all_args" > "$chroot_base/args.txt"
echo $$ > "$jail_root/firecracker.pid"
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
        run_dir: run_dir.clone(),
        uid: 3000,
        gid: 3000,
        bindings: Vec::new(),
        sockets: Vec::new(),
        resource_limits: crate::types::ResourceLimits::default(),
        new_pid_ns: false,
        new_net_ns: false,
        daemonize: false,
        new_cgroup_ns: false,
        cgroup_version,
        netns_path: None,
        seccomp_filter_path: None,
        stdio_log: None,
    };
    let plan = Plan::compute(&cfg).unwrap();
    let jail_path = run_dir
        .join("firecracker")
        .join("vm-cgroup-version")
        .join("root");
    let jail = MaterializedJail {
        plan,
        jail_path,
        bind_mounts: Vec::new(),
        created_dirs: Vec::new(),
        placeholder_files: Vec::new(),
    };

    let jailed = jail
        .launch(Path::new(JailerSocket::Firecracker.jail_path()))
        .unwrap();
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(jailed.jailer_pid() as i32),
        nix::sys::signal::Signal::SIGKILL,
    )
    .unwrap();
    std::fs::read_to_string(run_dir.join("args.txt")).unwrap()
}

#[test]
fn launch_with_cgroup_version_v2_passes_official_jailer_flag() {
    let args = launch_and_record_args(Some(CgroupVersion::V2));

    assert!(args.contains("--cgroup-version 2"), "{args}");
}

#[test]
fn launch_with_no_cgroup_version_omits_official_jailer_flag() {
    let args = launch_and_record_args(None);

    assert!(!args.contains("--cgroup-version"), "{args}");
}

#[test]
fn launch_with_minimal_config_omits_official_jailer_cgroup_placement_flags() {
    let args = launch_and_record_args(None);
    let argv: Vec<&str> = args.split_whitespace().collect();

    assert_no_cgroup_placement_flags(&argv, &args);
}

#[test]
fn launch_with_cgroup_version_v1_omits_official_jailer_flag() {
    let args = launch_and_record_args(Some(CgroupVersion::V1));

    assert!(!args.contains("--cgroup-version"), "{args}");
}

fn assert_no_cgroup_placement_flags(argv: &[&str], args: &str) {
    assert!(
        argv.iter()
            .all(|arg| *arg != "--cgroup" && !arg.starts_with("--cgroup=")),
        "{args}"
    );
    assert!(
        argv.iter()
            .all(|arg| *arg != "--parent-cgroup" && !arg.starts_with("--parent-cgroup=")),
        "{args}"
    );
}

#[test]
fn launch_redirects_stdio_and_passes_hardening_args() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = dir.path().join("vm-stdio");
    std::fs::create_dir_all(&run_dir).unwrap();
    let jailer_bin = dir.path().join("fake-jailer.sh");
    let jailer_harden_bin = dir.path().join("fake-harden.sh");
    let harden_args_path = run_dir.join("harden-args.txt");
    std::fs::write(
        &jailer_harden_bin,
        format!(
            r#"#!/bin/sh
printf '%s\n' "$*" > "{harden_args}"
jailer=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --jailer-bin) jailer="$2"; shift 2 ;;
    --uid) shift 2 ;;
    --gid) shift 2 ;;
    --rlimit) shift 2 ;;
    --new-cgroup-ns) shift ;;
    --new-net-ns) shift ;;
    --) shift; break ;;
    *) exit 64 ;;
  esac
done
exec "$jailer" "$@"
"#,
            harden_args = harden_args_path.display()
        ),
    )
    .unwrap();
    std::fs::write(
        &jailer_bin,
        r#"#!/bin/sh
if [ -n "$M80_JAILER_ENV_LEAK" ]; then
  echo env-leaked >&2
  exit 44
fi
id=
chroot_base=
exec_file=
all_args="$*"
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
printf '%s\n' "$all_args" > "$chroot_base/args.txt"
echo $$ > "$jail_root/firecracker.pid"
echo fake-firecracker-stdout
echo fake-firecracker-stderr >&2
/bin/sleep 30
"#,
    )
    .unwrap();
    let mut perms = std::fs::metadata(&jailer_bin).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&jailer_bin, perms).unwrap();
    let mut perms = std::fs::metadata(&jailer_harden_bin).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&jailer_harden_bin, perms).unwrap();

    let stdio_log = run_dir.join("console.log");
    std::fs::write(&stdio_log, b"old\n").unwrap();
    std::fs::set_permissions(&stdio_log, std::fs::Permissions::from_mode(0o644)).unwrap();
    let cfg = JailerConfig {
        jailer_bin,
        jailer_harden_bin: Some(jailer_harden_bin),
        firecracker_bin: PathBuf::from("/usr/bin/firecracker"),
        run_dir: run_dir.clone(),
        uid: 3000,
        gid: 3000,
        bindings: Vec::new(),
        sockets: Vec::new(),
        resource_limits: crate::types::ResourceLimits {
            no_file: 1024,
            fsize: Some(4096),
            nproc: Some(64),
            memlock: Some(0),
            address_space: Some(1_073_741_824),
            core: Some(0),
            stack: Some(8 * 1024 * 1024),
        },
        new_pid_ns: false,
        new_net_ns: true,
        daemonize: false,
        new_cgroup_ns: true,
        cgroup_version: None,
        netns_path: None,
        seccomp_filter_path: Some(PathBuf::from("firecracker-seccomp-filter.bin")),
        stdio_log: Some(stdio_log.clone()),
    };
    let plan = Plan::compute(&cfg).unwrap();
    let jail_path = run_dir.join("firecracker").join("vm-stdio").join("root");
    let jail = MaterializedJail {
        plan,
        jail_path,
        bind_mounts: Vec::new(),
        created_dirs: Vec::new(),
        placeholder_files: Vec::new(),
    };

    let _env_guard = EnvGuard::set("M80_JAILER_ENV_LEAK", "secret");
    let jailed = jail
        .launch(Path::new(JailerSocket::Firecracker.jail_path()))
        .unwrap();
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(jailed.jailer_pid as i32),
        nix::sys::signal::Signal::SIGKILL,
    )
    .unwrap();

    let log_mode = std::fs::metadata(&stdio_log).unwrap().permissions().mode() & 0o777;
    assert_eq!(log_mode, STDIO_LOG_FILE_MODE);

    let log = wait_for_log_contains(
        &stdio_log,
        &["fake-firecracker-stdout", "fake-firecracker-stderr"],
    );
    assert!(log.contains("fake-firecracker-stdout"), "{log}");
    assert!(log.contains("fake-firecracker-stderr"), "{log}");

    let args = std::fs::read_to_string(run_dir.join("args.txt")).unwrap();
    assert!(args.contains("--resource-limit no-file=1024"), "{args}");
    assert!(args.contains("--resource-limit fsize=4096"), "{args}");
    assert!(
        args.contains(
            "-- --api-sock firecracker.sock --seccomp-filter firecracker-seccomp-filter.bin"
        ),
        "{args}"
    );
    let harden_args = std::fs::read_to_string(harden_args_path).unwrap();
    assert!(harden_args.contains("--jailer-bin"), "{harden_args}");
    assert!(harden_args.contains("--uid 3000"), "{harden_args}");
    assert!(harden_args.contains("--gid 3000"), "{harden_args}");
    assert!(
        harden_args.contains("--rlimit no-file=1024"),
        "{harden_args}"
    );
    assert!(harden_args.contains("--rlimit fsize=4096"), "{harden_args}");
    assert!(harden_args.contains("--rlimit nproc=64"), "{harden_args}");
    assert!(harden_args.contains("--rlimit memlock=0"), "{harden_args}");
    assert!(
        harden_args.contains("--rlimit as=1073741824"),
        "{harden_args}"
    );
    assert!(harden_args.contains("--rlimit core=0"), "{harden_args}");
    assert!(
        harden_args.contains("--rlimit stack=8388608"),
        "{harden_args}"
    );
    assert!(harden_args.contains("--new-cgroup-ns"), "{harden_args}");
    assert!(harden_args.contains("--new-net-ns"), "{harden_args}");
}

#[test]
fn stdio_log_copier_caps_persisted_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("console.log");

    copy_limited_stdio_to_log(Cursor::new(b"abcdef"), &log_path, 4).unwrap();

    assert_eq!(std::fs::read(&log_path).unwrap(), b"abcd");
}

#[test]
fn stdio_log_copier_respects_existing_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("console.log");
    std::fs::write(&log_path, b"old").unwrap();

    copy_limited_stdio_to_log(Cursor::new(b"abcdef"), &log_path, 5).unwrap();

    assert_eq!(std::fs::read(&log_path).unwrap(), b"oldab");
}

#[test]
fn stdio_log_copiers_share_one_byte_cap() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("console.log");
    let written = Arc::new(Mutex::new(0));

    copy_limited_stdio_to_log_with_counter(
        Cursor::new(b"abcd"),
        &log_path,
        6,
        Arc::clone(&written),
    )
    .unwrap();
    copy_limited_stdio_to_log_with_counter(Cursor::new(b"efgh"), &log_path, 6, written).unwrap();

    assert_eq!(std::fs::read(&log_path).unwrap(), b"abcdef");
}

#[test]
fn launch_with_new_pid_ns_reaps_jailer_parent() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = dir.path().join("vm-newpid");
    std::fs::create_dir_all(&run_dir).unwrap();
    let jailer_bin = dir.path().join("fake-jailer-newpid.sh");
    std::fs::write(
        &jailer_bin,
        r#"#!/bin/sh
id=
chroot_base=
exec_file=
new_pid_ns=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --id) id="$2"; shift 2 ;;
    --chroot-base-dir) chroot_base="$2"; shift 2 ;;
    --exec-file) exec_file="$2"; shift 2 ;;
    --new-pid-ns) new_pid_ns=1; shift ;;
    --) shift; break ;;
    *) shift ;;
  esac
done
exec_base="${exec_file##*/}"
jail_root="$chroot_base/$exec_base/$id/root"
/bin/mkdir -p "$jail_root"
echo $$ > "$jail_root/firecracker.pid"
if [ "$new_pid_ns" -eq 1 ]; then
  exit 0
fi
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
        run_dir: run_dir.clone(),
        uid: 3000,
        gid: 3000,
        bindings: Vec::new(),
        sockets: Vec::new(),
        resource_limits: crate::types::ResourceLimits::default(),
        new_pid_ns: true,
        new_net_ns: false,
        daemonize: false,
        new_cgroup_ns: false,
        cgroup_version: None,
        netns_path: None,
        seccomp_filter_path: None,
        stdio_log: None,
    };
    let plan = Plan::compute(&cfg).unwrap();
    let jail_path = run_dir.join("firecracker").join("vm-newpid").join("root");
    let jail = MaterializedJail {
        plan,
        jail_path,
        bind_mounts: Vec::new(),
        created_dirs: Vec::new(),
        placeholder_files: Vec::new(),
    };

    let jailed = jail
        .launch(Path::new(JailerSocket::Firecracker.jail_path()))
        .unwrap();
    assert_eq!(jailed.jailer_pid(), 0);

    let state = std::fs::read_to_string(run_dir.join("jailer-state.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&state).unwrap();
    assert_eq!(parsed["jailer_pid"], 0);
}

#[test]
fn launch_with_daemonize_reaps_jailer_parent_and_records_daemon_pid() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = dir.path().join("vm-daemon");
    std::fs::create_dir_all(&run_dir).unwrap();
    let jailer_bin = dir.path().join("fake-jailer-daemon.sh");
    std::fs::write(
        &jailer_bin,
        r#"#!/bin/sh
id=
chroot_base=
exec_file=
daemonize=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --id) id="$2"; shift 2 ;;
    --chroot-base-dir) chroot_base="$2"; shift 2 ;;
    --exec-file) exec_file="$2"; shift 2 ;;
    --daemonize) daemonize=1; shift ;;
    --) shift; break ;;
    *) shift ;;
  esac
done
exec_base="${exec_file##*/}"
jail_root="$chroot_base/$exec_base/$id/root"
/bin/mkdir -p "$jail_root"
if [ "$daemonize" -eq 1 ]; then
  /bin/sleep 30 &
  echo $! > "$jail_root/firecracker.pid"
  exit 0
fi
echo $$ > "$jail_root/firecracker.pid"
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
        run_dir: run_dir.clone(),
        uid: 3000,
        gid: 3000,
        bindings: Vec::new(),
        sockets: Vec::new(),
        resource_limits: crate::types::ResourceLimits::default(),
        new_pid_ns: false,
        new_net_ns: false,
        daemonize: true,
        new_cgroup_ns: false,
        cgroup_version: None,
        netns_path: None,
        seccomp_filter_path: None,
        stdio_log: None,
    };
    let plan = Plan::compute(&cfg).unwrap();
    let jail_path = run_dir.join("firecracker").join("vm-daemon").join("root");
    let jail = MaterializedJail {
        plan,
        jail_path,
        bind_mounts: Vec::new(),
        created_dirs: Vec::new(),
        placeholder_files: Vec::new(),
    };

    let jailed = jail
        .launch(Path::new(JailerSocket::Firecracker.jail_path()))
        .unwrap();
    assert_eq!(jailed.jailer_pid(), 0);
    assert!(
        std::path::Path::new(&format!("/proc/{}", jailed.firecracker_pid())).exists(),
        "daemonized firecracker pid must remain live after jailer parent exits"
    );

    let state = std::fs::read_to_string(run_dir.join("jailer-state.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&state).unwrap();
    assert_eq!(parsed["jailer_pid"], 0);
    assert_eq!(parsed["firecracker_pid"], jailed.firecracker_pid());

    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(jailed.firecracker_pid() as i32),
        nix::sys::signal::Signal::SIGKILL,
    )
    .unwrap();
}

#[test]
fn launch_without_stdio_log_uses_dev_null_stdio() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = dir.path().join("vm-null-stdio");
    std::fs::create_dir_all(&run_dir).unwrap();
    let jailer_bin = dir.path().join("fake-jailer-null-stdio.sh");
    let marker = run_dir.join("stdio.txt");
    std::fs::write(
        &jailer_bin,
        format!(
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
exec_base="${{exec_file##*/}}"
jail_root="$chroot_base/$exec_base/$id/root"
/bin/mkdir -p "$jail_root"
stdio="$(readlink /proc/$$/fd/0) $(readlink /proc/$$/fd/1) $(readlink /proc/$$/fd/2)"
printf '%s\n' "$stdio" > "{marker}"
echo $$ > "$jail_root/firecracker.pid"
/bin/sleep 30
"#,
            marker = marker.display()
        ),
    )
    .unwrap();
    let mut perms = std::fs::metadata(&jailer_bin).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&jailer_bin, perms).unwrap();

    let cfg = JailerConfig {
        jailer_bin,
        jailer_harden_bin: None,
        firecracker_bin: PathBuf::from("/usr/bin/firecracker"),
        run_dir: run_dir.clone(),
        uid: 3000,
        gid: 3000,
        bindings: Vec::new(),
        sockets: Vec::new(),
        resource_limits: crate::types::ResourceLimits::default(),
        new_pid_ns: false,
        new_net_ns: false,
        daemonize: false,
        new_cgroup_ns: false,
        cgroup_version: None,
        netns_path: None,
        seccomp_filter_path: None,
        stdio_log: None,
    };
    let plan = Plan::compute(&cfg).unwrap();
    let jail_path = run_dir
        .join("firecracker")
        .join("vm-null-stdio")
        .join("root");
    let jail = MaterializedJail {
        plan,
        jail_path,
        bind_mounts: Vec::new(),
        created_dirs: Vec::new(),
        placeholder_files: Vec::new(),
    };

    let jailed = jail
        .launch(Path::new(JailerSocket::Firecracker.jail_path()))
        .unwrap();
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(jailed.jailer_pid() as i32),
        nix::sys::signal::Signal::SIGKILL,
    )
    .unwrap();

    let stdio = std::fs::read_to_string(marker).unwrap();
    assert_eq!(
        stdio.split_whitespace().collect::<Vec<_>>(),
        vec!["/dev/null", "/dev/null", "/dev/null"]
    );
}

#[test]
fn validate_netns_path_rejects_regular_file() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let err = validate_netns_path(file.path()).unwrap_err();
    assert!(
        matches!(err, JailerError::InvalidNetns { .. }),
        "expected InvalidNetns for regular file, got {err:?}"
    );
}

#[test]
fn validate_netns_path_rejects_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    let link = dir.path().join("link");
    std::fs::write(&target, b"not a namespace").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let err = validate_netns_path(&link).unwrap_err();
    assert!(
        matches!(err, JailerError::Io { .. }),
        "expected O_NOFOLLOW open failure for symlink, got {err:?}"
    );
}
