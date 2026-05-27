//! Root-only integration tests for inherited process hardening.
//!
//! Run manually:
//!
//! ```sh
//! sudo cargo test -p m80-jailer-harden --features no-systemd-launch --test integration_root -- --ignored
//! ```

use std::io::Write;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use std::{fs, io, thread};

use nix::fcntl::{fcntl, FcntlArg, FdFlag};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;

const M80_CGROUP_PARENT: &str = "/sys/fs/cgroup/m80-firecracker";

#[test]
#[ignore = "requires-root"]
fn wrapper_applies_inherited_hardening_before_exec() {
    let wrapper = env!("CARGO_BIN_EXE_m80-jailer-harden");
    let inherited_file = tempfile::NamedTempFile::new().expect("inherited fd file");
    let inherited_fd = inherited_file.as_file().as_raw_fd();
    let inherited_path = inherited_file.path().to_string_lossy();
    let current_flags =
        FdFlag::from_bits_truncate(fcntl(inherited_fd, FcntlArg::F_GETFD).expect("get fd flags"));
    fcntl(
        inherited_fd,
        FcntlArg::F_SETFD(current_flags - FdFlag::FD_CLOEXEC),
    )
    .expect("clear cloexec");

    let output = Command::new(wrapper)
        .env("M80_SECRET_SHOULD_NOT_LEAK", "secret")
        .args([
            "--jailer-bin",
            "/bin/sh",
            "--uid",
            "3000",
            "--gid",
            "3000",
            "--",
            "-c",
            &format!(
                r#"if [ -n "$M80_SECRET_SHOULD_NOT_LEAK" ]; then echo env-leaked; exit 44; fi
if target=$(readlink /proc/self/fd/{inherited_fd} 2>/dev/null) && [ "$target" = "{inherited_path}" ]; then echo fd-leaked; exit 45; fi
printf 'UMASK=%s\n' "$(umask)"
grep -E '^(Groups|NoNewPrivs|CapInh|CapAmb|CapBnd|CapPrm|CapEff|SigBlk):' /proc/self/status
"#,
                inherited_fd = inherited_fd,
                inherited_path = inherited_path
            ),
        ])
        .output()
        .expect("run wrapper");

    assert!(
        output.status.success(),
        "status={} stdout=\n{}\nstderr=\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("UMASK=0077"), "{stdout}");
    assert!(stdout.contains("NoNewPrivs:\t1"), "{stdout}");
    assert!(stdout.contains("CapInh:\t0000000000000000"), "{stdout}");
    assert!(stdout.contains("CapAmb:\t0000000000000000"), "{stdout}");
    assert!(stdout.contains("SigBlk:\t0000000000000000"), "{stdout}");
    for label in ["CapBnd", "CapPrm", "CapEff"] {
        let mask = status_hex_value(&stdout, label);
        assert_eq!(
            mask & official_jailer_cap_mask(),
            official_jailer_cap_mask(),
            "{label} missing official jailer caps:\n{stdout}"
        );
        assert_eq!(
            mask & forbidden_pre_jailer_cap_mask(),
            0,
            "{label} retained forbidden pre-jailer caps:\n{stdout}"
        );
    }

    let groups = stdout
        .lines()
        .find(|line| line.starts_with("Groups:"))
        .unwrap_or_else(|| panic!("missing Groups line:\n{stdout}"));
    assert_eq!(groups.trim(), "Groups:", "{stdout}");
}

fn status_hex_value(stdout: &str, label: &str) -> u64 {
    let prefix = format!("{label}:\t");
    let line = stdout
        .lines()
        .find(|line| line.starts_with(&prefix))
        .unwrap_or_else(|| panic!("missing {label} line:\n{stdout}"));
    u64::from_str_radix(line[prefix.len()..].trim(), 16)
        .unwrap_or_else(|err| panic!("parse {label} from {line:?}: {err}"))
}

fn official_jailer_cap_mask() -> u64 {
    cap_mask(&[0, 1, 6, 7, 18, 21, 27])
}

fn forbidden_pre_jailer_cap_mask() -> u64 {
    cap_mask(&[3, 5, 8, 12, 16, 17, 19])
}

fn cap_mask(indices: &[u8]) -> u64 {
    indices.iter().fold(0, |mask, index| mask | (1u64 << index))
}

#[test]
#[ignore = "requires-root requires-cgroup-v2"]
fn wrapper_new_cgroup_ns_roots_proc_self_cgroup() {
    let wrapper = env!("CARGO_BIN_EXE_m80-jailer-harden");
    let vm_id = format!("m80-cgroup-ns-{}", std::process::id());
    let leaf = Path::new(M80_CGROUP_PARENT).join(vm_id);
    let guard = RealCgroupLeafGuard::create(leaf.clone());

    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg(
            "read _; exec \"$1\" --jailer-bin /bin/sh --uid 3000 --gid 3000 \
             --new-cgroup-ns -- -c 'cat /proc/self/cgroup'",
        )
        .arg("m80-cgroup-ns-test")
        .arg(wrapper)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn waiting wrapper process");

    write_cgroup_file(&leaf.join("cgroup.procs"), &format!("{}\n", child.id()));
    child
        .stdin
        .as_mut()
        .expect("child stdin")
        .write_all(b"go\n")
        .expect("start wrapper process");

    let output = wait_for_child_output(child, Duration::from_secs(10));
    assert!(
        output.status.success(),
        "status={} stdout=\n{}\nstderr=\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout.trim(),
        "0::/",
        "new cgroup namespace must hide host leaf {}; stdout={stdout:?}",
        leaf.display()
    );

    drop(guard);
}

struct RealCgroupLeafGuard {
    leaf: PathBuf,
}

impl RealCgroupLeafGuard {
    fn create(leaf: PathBuf) -> Self {
        if leaf.exists() {
            kill_pids_in_cgroup(&leaf);
            wait_for_empty_cgroup(&leaf);
            fs::remove_dir(&leaf).expect("remove stale cgroup namespace test leaf");
        }
        fs::create_dir_all(M80_CGROUP_PARENT).expect("create m80 cgroup parent");
        fs::create_dir(&leaf).expect("create cgroup namespace test leaf");
        Self { leaf }
    }
}

impl Drop for RealCgroupLeafGuard {
    fn drop(&mut self) {
        kill_pids_in_cgroup(&self.leaf);
        wait_for_empty_cgroup(&self.leaf);
        let _ = fs::remove_dir(&self.leaf);
    }
}

fn write_cgroup_file(path: &Path, value: &str) {
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|mut file| file.write_all(value.as_bytes()))
        .unwrap_or_else(|err| panic!("write {}: {err}", path.display()));
}

fn wait_for_child_output(mut child: Child, timeout: Duration) -> std::process::Output {
    let started = Instant::now();
    loop {
        if child.try_wait().expect("poll child").is_some() {
            return child.wait_with_output().expect("collect child output");
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            panic!("wrapper process did not finish within {timeout:?}");
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn kill_pids_in_cgroup(leaf: &Path) {
    for pid in read_cgroup_pids(leaf) {
        match kill(Pid::from_raw(pid), Signal::SIGKILL) {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => {}
            Err(err) => panic!("SIGKILL {pid}: {err}"),
        }
    }
}

fn wait_for_empty_cgroup(leaf: &Path) {
    let started = Instant::now();
    while !read_cgroup_pids(leaf).is_empty() {
        assert!(
            started.elapsed() <= Duration::from_secs(5),
            "{} still has live pids: {:?}",
            leaf.display(),
            read_cgroup_pids(leaf)
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn read_cgroup_pids(leaf: &Path) -> Vec<i32> {
    match fs::read_to_string(leaf.join("cgroup.procs")) {
        Ok(content) => content
            .split_whitespace()
            .map(|pid| pid.parse::<i32>().expect("numeric cgroup pid"))
            .collect(),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(err) => panic!("read {}/cgroup.procs: {err}", leaf.display()),
    }
}
