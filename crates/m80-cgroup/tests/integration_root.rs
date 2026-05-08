//! Real-host integration tests; `#[ignore]`d so CI without cgroup v2 skips.
//! Run with: `sudo cargo test -p m80-cgroup -- --ignored`.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;

const CGROUP_ROOT: &str = "/sys/fs/cgroup";
const M80_CGROUP_PARENT: &str = "/sys/fs/cgroup/m80-firecracker";
const PIDS_MAX_ENFORCEMENT_LIMIT: u32 = 128;

const PYTHON_FORK_UNTIL_EAGAIN: &str = r#"
import errno
import os
import sys
import time

cgroup_path = os.environ["M80_TEST_CGROUP_PATH"]
result_path = os.environ["M80_TEST_RESULT_PATH"]

sys.stdin.readline()

children = []
while len(children) < 512:
    try:
        pid = os.fork()
    except OSError as exc:
        events = {}
        with open(os.path.join(cgroup_path, "pids.events"), encoding="utf-8") as events_file:
            for line in events_file:
                parts = line.split()
                if len(parts) == 2:
                    events[parts[0]] = parts[1]
        with open(result_path, "w", encoding="utf-8") as result:
            result.write(f"errno={exc.errno}\n")
            result.write(f"pids.current={open(os.path.join(cgroup_path, 'pids.current'), encoding='utf-8').read().strip()}\n")
            result.write(f"children={len(children)}\n")
            result.write(f"pids.events.max={events.get('max', '0')}\n")
        sys.exit(12 if exc.errno == errno.EAGAIN else 70)
    if pid == 0:
        time.sleep(30)
        os._exit(0)
    children.append(pid)

with open(result_path, "w", encoding="utf-8") as result:
    result.write("errno=0\n")
    result.write(f"children={len(children)}\n")
sys.exit(99)
"#;

#[test]
#[ignore]
fn probe_returns_ok_on_unified_v2_host() {
    m80_cgroup::Subtree::probe().expect("probe() must return Ok on a unified-v2 host");
}

#[test]
#[ignore = "requires root and a writable cgroup v2 hierarchy"]
fn cgroup_pids_max_enforced_against_fork_bomb() {
    m80_cgroup::Subtree::probe().expect("probe() must return Ok on a unified-v2 host");

    let vm_id = format!("m80-pids-eagain-{}", std::process::id());
    let result_dir = tempfile::tempdir().expect("tempdir");
    let result_path = result_dir.path().join("fork-result.txt");
    let leaf = prepare_pids_enforcement_cgroup(&vm_id);
    let guard = CgroupLeafGuard { leaf: leaf.clone() };

    let mut child = Command::new("python3")
        .arg("-c")
        .arg(PYTHON_FORK_UNTIL_EAGAIN)
        .env("M80_TEST_CGROUP_PATH", &leaf)
        .env("M80_TEST_RESULT_PATH", &result_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn fork workload");

    write_cgroup_file(&leaf.join("cgroup.procs"), &format!("{}\n", child.id()));
    child
        .stdin
        .as_mut()
        .expect("child stdin")
        .write_all(b"go\n")
        .expect("start fork workload");

    let status = wait_for_child(&mut child, Duration::from_secs(10));

    assert_eq!(
        status.code(),
        Some(12),
        "fork workload must map kernel EAGAIN to exit 12; status={status:?}"
    );

    let result = read_key_values(&result_path);
    assert_eq!(result.get("errno").map(String::as_str), Some("11"));
    assert_eq!(
        result.get("pids.current").map(String::as_str),
        Some(PIDS_MAX_ENFORCEMENT_LIMIT.to_string().as_str()),
        "pids.current at EAGAIN must equal pids.max; result={result:?}"
    );
    assert_eq!(
        result.get("children").map(String::as_str),
        Some((PIDS_MAX_ENFORCEMENT_LIMIT - 1).to_string().as_str()),
        "one enrolled python parent plus children should fill pids.max; result={result:?}"
    );
    assert_ne!(
        result.get("pids.events.max").map(String::as_str),
        Some("0"),
        "pids.events max counter must record the rejected fork; result={result:?}"
    );

    drop(guard);
}

struct CgroupLeafGuard {
    leaf: PathBuf,
}

impl Drop for CgroupLeafGuard {
    fn drop(&mut self) {
        kill_pids_in_cgroup(&self.leaf);
        wait_for_empty_cgroup(&self.leaf);
        let _ = fs::remove_dir(&self.leaf);
    }
}

fn prepare_pids_enforcement_cgroup(vm_id: &str) -> PathBuf {
    let root = Path::new(CGROUP_ROOT);
    let parent = Path::new(M80_CGROUP_PARENT);
    let leaf = parent.join(vm_id);

    if leaf.exists() {
        kill_pids_in_cgroup(&leaf);
        wait_for_empty_cgroup(&leaf);
        fs::remove_dir(&leaf).expect("remove stale pids test cgroup");
    }

    enable_pids_controller(root);
    fs::create_dir_all(parent).expect("create m80 cgroup parent");
    enable_pids_controller(parent);
    fs::create_dir(&leaf).expect("create pids test cgroup leaf");
    write_cgroup_file(
        &leaf.join("pids.max"),
        &format!("{PIDS_MAX_ENFORCEMENT_LIMIT}\n"),
    );
    leaf
}

fn enable_pids_controller(path: &Path) {
    let controllers =
        fs::read_to_string(path.join("cgroup.controllers")).expect("read cgroup.controllers");
    assert!(
        controllers
            .split_whitespace()
            .any(|controller| controller == "pids"),
        "{} must expose the pids controller: {controllers:?}",
        path.display()
    );
    write_cgroup_file(&path.join("cgroup.subtree_control"), "+pids\n");
}

fn write_cgroup_file(path: &Path, value: &str) {
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|mut file| file.write_all(value.as_bytes()))
        .unwrap_or_else(|err| panic!("write {}: {err}", path.display()));
}

fn wait_for_child(child: &mut Child, timeout: Duration) -> std::process::ExitStatus {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("poll child") {
            return status;
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            panic!("fork workload did not finish within {timeout:?}");
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn read_key_values(path: &Path) -> BTreeMap<String, String> {
    fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
        .lines()
        .map(|line| {
            line.split_once('=')
                .unwrap_or_else(|| panic!("result line must be key=value: {line:?}"))
        })
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

fn kill_pids_in_cgroup(leaf: &Path) {
    for pid in read_cgroup_pids(leaf) {
        match kill(Pid::from_raw(pid), Signal::SIGKILL) {
            Ok(()) | Err(Errno::ESRCH) => {}
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
