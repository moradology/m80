use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use m80_jailer::{JailerConfig, Plan};

#[test]
fn minimal_launch_emits_no_official_jailer_cgroup_placement_flags() {
    let args = launch_and_record_args();
    let argv: Vec<&str> = args.split_whitespace().collect();

    assert_no_cgroup_placement_flags(&argv, &args);
}

fn launch_and_record_args() -> String {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = dir.path().join("vm-no-cgroup-flag");
    std::fs::create_dir_all(&run_dir).unwrap();
    let jailer_bin = dir.path().join("fake-jailer-no-cgroup-flag.sh");
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
        resource_limits: m80_jailer::ResourceLimits::default(),
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
    let jail = m80_jailer::materialized_jail_for_test(plan);

    let jailed = jail.launch(Path::new("firecracker.sock")).unwrap();
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(jailed.jailer_pid() as i32),
        nix::sys::signal::Signal::SIGKILL,
    )
    .unwrap();
    std::fs::read_to_string(run_dir.join("args.txt")).unwrap()
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
