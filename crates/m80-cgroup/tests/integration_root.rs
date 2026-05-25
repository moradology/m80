//! Real-host integration tests; `#[ignore]`d so CI without cgroup v2 skips.
//! Run with: `sudo cargo test -p m80-cgroup -- --ignored`.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::sys::signal::{kill, Signal};
use nix::sys::stat::{major, minor};
use nix::unistd::Pid;

const CGROUP_ROOT: &str = "/sys/fs/cgroup";
const M80_CGROUP_PARENT: &str = "/sys/fs/cgroup/m80-firecracker";
const PIDS_MAX_ENFORCEMENT_LIMIT: u32 = 128;
const IO_MAX_WBPS: u64 = 1_048_576;
const IO_MAX_WRITE_BYTES: u64 = 16 * 1_048_576;
const IO_MAX_ALLOWED_BPS: f64 = 1.5 * 1_048_576.0;

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

const PYTHON_TRY_WIDEN_AFFINITY: &str = r#"
import os
import sys
import time

result_path = os.environ["M80_TEST_RESULT_PATH"]

sys.stdin.readline()

try:
    os.sched_setaffinity(0, set(range(os.cpu_count() or 1)))
    affinity_errno = 0
except OSError as exc:
    affinity_errno = exc.errno

allowed = ""
with open(f"/proc/{os.getpid()}/status", encoding="utf-8") as status:
    for line in status:
        if line.startswith("Cpus_allowed_list:"):
            allowed = line.split(":", 1)[1].strip()
            break

with open(result_path, "w", encoding="utf-8") as result:
    result.write(f"affinity_errno={affinity_errno}\n")
    result.write(f"cpus_allowed_list={allowed}\n")

time.sleep(30)
"#;

#[test]
#[ignore = "requires-root requires-cgroup-v2"]
fn probe_returns_ok_on_unified_v2_host() {
    m80_cgroup::Subtree::probe().expect("probe() must return Ok on a unified-v2 host");
}

#[test]
#[ignore = "requires-root requires-cgroup-v2"]
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

#[test]
#[ignore = "requires-root requires-cgroup-v2"]
fn cgroup_cpuset_pinning_actually_constrains_affinity() {
    m80_cgroup::Subtree::probe().expect("probe() must return Ok on a unified-v2 host");

    let vm_id = format!("m80-cpuset-{}", std::process::id());
    let result_dir = tempfile::tempdir().expect("tempdir");
    let result_path = result_dir.path().join("affinity-result.txt");
    let leaf = prepare_cpuset_enforcement_cgroup(&vm_id);
    let guard = CgroupLeafGuard { leaf: leaf.clone() };

    let pinned_cpu = fs::read_to_string(leaf.join("cpuset.cpus"))
        .expect("read pinned cpuset")
        .trim()
        .to_owned();
    let mut child = Command::new("python3")
        .arg("-c")
        .arg(PYTHON_TRY_WIDEN_AFFINITY)
        .env("M80_TEST_RESULT_PATH", &result_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn affinity workload");

    write_cgroup_file(&leaf.join("cgroup.procs"), &format!("{}\n", child.id()));
    child
        .stdin
        .as_mut()
        .expect("child stdin")
        .write_all(b"go\n")
        .expect("start affinity workload");

    wait_for_file(&result_path, Duration::from_secs(10));
    let result = read_key_values(&result_path);
    assert_eq!(
        result.get("cpus_allowed_list").map(String::as_str),
        Some(pinned_cpu.as_str()),
        "cpuset must constrain affinity after workload tries to widen it; result={result:?}"
    );

    drop(guard);
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
#[ignore = "requires-root requires-cgroup-v2"]
fn cgroup_io_max_throttles_disk_writes() {
    m80_cgroup::Subtree::probe().expect("probe() must return Ok on a unified-v2 host");

    let vm_id = format!("m80-io-max-{}", std::process::id());
    let write_dir = tempfile::tempdir_in("/var/tmp")
        .or_else(|_| tempfile::tempdir())
        .expect("temp dir for direct-write workload");
    let output_path = write_dir.path().join("io-max-write.bin");
    let device = device_major_minor(write_dir.path());
    let leaf = prepare_io_max_enforcement_cgroup(&vm_id, device);
    let guard = CgroupLeafGuard { leaf: leaf.clone() };

    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg(
            "read _; exec dd if=/dev/zero of=\"$1\" bs=1M count=16 \
             oflag=direct conv=fdatasync status=none",
        )
        .arg("m80-io-max-test")
        .arg(&output_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn direct-write workload");

    write_cgroup_file(&leaf.join("cgroup.procs"), &format!("{}\n", child.id()));
    let started = Instant::now();
    child
        .stdin
        .as_mut()
        .expect("child stdin")
        .write_all(b"go\n")
        .expect("start direct-write workload");
    let status = wait_for_child(&mut child, Duration::from_secs(45));
    let elapsed = started.elapsed();

    assert!(
        status.success(),
        "direct-write workload must complete successfully: {status:?}"
    );
    let written = fs::metadata(&output_path)
        .expect("stat direct-write output")
        .len();
    assert_eq!(written, IO_MAX_WRITE_BYTES);
    let bytes_per_second = written as f64 / elapsed.as_secs_f64();
    assert!(
        bytes_per_second <= IO_MAX_ALLOWED_BPS,
        "io.max wbps={IO_MAX_WBPS} must throttle throughput below 1.5MiB/s; \
         wrote {written} bytes in {elapsed:?} ({bytes_per_second:.0} B/s)"
    );
    assert!(
        io_stat_wbytes(&leaf, device) > 0,
        "io.stat must record writes for device {}:{}",
        device.0,
        device.1
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

    enable_controller(root, "pids");
    fs::create_dir_all(parent).expect("create m80 cgroup parent");
    enable_controller(parent, "pids");
    fs::create_dir(&leaf).expect("create pids test cgroup leaf");
    write_cgroup_file(
        &leaf.join("pids.max"),
        &format!("{PIDS_MAX_ENFORCEMENT_LIMIT}\n"),
    );
    leaf
}

fn prepare_cpuset_enforcement_cgroup(vm_id: &str) -> PathBuf {
    let root = Path::new(CGROUP_ROOT);
    let parent = Path::new(M80_CGROUP_PARENT);
    let leaf = parent.join(vm_id);

    if leaf.exists() {
        kill_pids_in_cgroup(&leaf);
        wait_for_empty_cgroup(&leaf);
        fs::remove_dir(&leaf).expect("remove stale cpuset test cgroup");
    }

    enable_controller(root, "cpuset");
    fs::create_dir_all(parent).expect("create m80 cgroup parent");

    let parent_cpus = fs::read_to_string(parent.join("cpuset.cpus.effective"))
        .or_else(|_| fs::read_to_string(root.join("cpuset.cpus.effective")))
        .expect("read effective parent cpuset.cpus");
    let parent_mems = fs::read_to_string(parent.join("cpuset.mems.effective"))
        .or_else(|_| fs::read_to_string(root.join("cpuset.mems.effective")))
        .expect("read effective parent cpuset.mems");
    write_cgroup_file(
        &parent.join("cpuset.cpus"),
        &format!("{}\n", parent_cpus.trim()),
    );
    write_cgroup_file(
        &parent.join("cpuset.mems"),
        &format!("{}\n", parent_mems.trim()),
    );
    enable_controller(parent, "cpuset");

    fs::create_dir(&leaf).expect("create cpuset test cgroup leaf");
    let pinned_cpu = first_cpuset_member(parent_cpus.trim());
    let pinned_mem = first_cpuset_member(parent_mems.trim());
    write_cgroup_file(&leaf.join("cpuset.cpus"), &format!("{pinned_cpu}\n"));
    write_cgroup_file(&leaf.join("cpuset.mems"), &format!("{pinned_mem}\n"));
    leaf
}

fn prepare_io_max_enforcement_cgroup(vm_id: &str, device: (u64, u64)) -> PathBuf {
    let root = Path::new(CGROUP_ROOT);
    let parent = Path::new(M80_CGROUP_PARENT);
    let leaf = parent.join(vm_id);

    if leaf.exists() {
        kill_pids_in_cgroup(&leaf);
        wait_for_empty_cgroup(&leaf);
        fs::remove_dir(&leaf).expect("remove stale io.max test cgroup");
    }

    enable_controller(root, "io");
    fs::create_dir_all(parent).expect("create m80 cgroup parent");
    enable_controller(parent, "io");
    fs::create_dir(&leaf).expect("create io.max test cgroup leaf");
    write_cgroup_file(
        &leaf.join("io.max"),
        &format!("{}:{} wbps={IO_MAX_WBPS}\n", device.0, device.1),
    );
    leaf
}

fn enable_controller(path: &Path, controller: &str) {
    let controllers =
        fs::read_to_string(path.join("cgroup.controllers")).expect("read cgroup.controllers");
    assert!(
        controllers
            .split_whitespace()
            .any(|candidate| candidate == controller),
        "{} must expose the {controller} controller: {controllers:?}",
        path.display()
    );
    write_cgroup_file(
        &path.join("cgroup.subtree_control"),
        &format!("+{controller}\n"),
    );
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

fn wait_for_file(path: &Path, timeout: Duration) {
    let started = Instant::now();
    while !path.exists() {
        assert!(
            started.elapsed() <= timeout,
            "{} was not created within {timeout:?}",
            path.display()
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn first_cpuset_member(value: &str) -> String {
    let first_range = value
        .split(',')
        .next()
        .unwrap_or_else(|| panic!("cpuset value must not be empty: {value:?}"));
    first_range
        .split('-')
        .next()
        .filter(|member| !member.is_empty())
        .unwrap_or_else(|| panic!("cpuset range must start with a CPU/mem id: {value:?}"))
        .to_owned()
}

fn device_major_minor(path: &Path) -> (u64, u64) {
    let dev = fs::metadata(path)
        .unwrap_or_else(|err| panic!("stat {}: {err}", path.display()))
        .dev();
    (major(dev), minor(dev))
}

fn io_stat_wbytes(leaf: &Path, device: (u64, u64)) -> u64 {
    let stat = fs::read_to_string(leaf.join("io.stat")).expect("read io.stat");
    let prefix = format!("{}:{}", device.0, device.1);
    stat.lines()
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            if parts.next()? != prefix {
                return None;
            }
            parts.find_map(|part| {
                let (key, value) = part.split_once('=')?;
                (key == "wbytes").then(|| value.parse::<u64>().expect("numeric wbytes"))
            })
        })
        .unwrap_or(0)
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
