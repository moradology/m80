use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use m80_jailer::{JailerConfig, Plan, ResourceLimits};

fn current_or_root_test_uid_gid() -> (u32, u32) {
    let uid = nix::unistd::Uid::current().as_raw();
    let gid = nix::unistd::Gid::current().as_raw();
    if uid == 0 || gid == 0 {
        (1, 1)
    } else {
        (uid, gid)
    }
}

#[test]
#[ignore = "requires-root"]
fn launch_observes_pid_file_without_fixed_twenty_five_ms_floor() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = dir.path().join("vm-pid-backoff");
    std::fs::create_dir_all(&run_dir).unwrap();
    let jailer_bin = dir.path().join("fake-jailer-pid-backoff.sh");
    write_delayed_pid_jailer(&jailer_bin);

    let (uid, gid) = current_or_root_test_uid_gid();
    let cfg = JailerConfig {
        jailer_bin,
        jailer_harden_bin: None,
        firecracker_bin: PathBuf::from("/usr/bin/firecracker"),
        run_dir: run_dir.clone(),
        uid,
        gid,
        bindings: Vec::new(),
        sockets: Vec::new(),
        resource_limits: ResourceLimits::default(),
        new_pid_ns: false,
        new_net_ns: false,
        daemonize: false,
        new_cgroup_ns: false,
        cgroup_version: None,
        netns_path: None,
        seccomp_filter_path: None,
        stdio_log: None,
    };
    let jail = Plan::compute(&cfg).unwrap().materialize().unwrap();

    let start = Instant::now();
    let jailed = jail.launch(Path::new("firecracker.sock")).unwrap();
    let elapsed = start.elapsed();

    assert_eq!(jailed.jailer_pid(), jailed.firecracker_pid());
    assert!(
        elapsed < Duration::from_millis(25),
        "launch should not pay the old fixed 25ms pid-file sleep; elapsed={elapsed:?}"
    );

    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(jailed.jailer_pid() as i32),
        nix::sys::signal::Signal::SIGKILL,
    )
    .unwrap();
}

fn write_delayed_pid_jailer(path: &Path) {
    std::fs::write(
        path,
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
/bin/sleep 0.002
echo $$ > "$jail_root/firecracker.pid"
/bin/sleep 30
"#,
    )
    .unwrap();
    let mut perms = std::fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).unwrap();
}
