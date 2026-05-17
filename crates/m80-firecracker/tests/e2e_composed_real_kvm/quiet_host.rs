use std::path::Path;
use std::path::PathBuf;

use sha2::{Digest as _, Sha256};

#[derive(Debug)]
pub(super) struct FirecrackerProcess {
    pid: u32,
    argv: Vec<String>,
}

impl FirecrackerProcess {
    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "pid": self.pid,
            "argv": self.argv,
        })
    }
}

#[allow(dead_code)]
pub(super) fn assert_or_record_quiet_host(allow_env: &str) -> (bool, Vec<FirecrackerProcess>) {
    assert_or_record_quiet_host_for("composed e2e", allow_env)
}

pub(super) fn assert_or_record_quiet_host_for(
    label: &str,
    allow_env: &str,
) -> (bool, Vec<FirecrackerProcess>) {
    let processes = preexisting_firecracker_processes();
    let allow_other_firecracker_vms = env_bool(allow_env);
    if !allow_other_firecracker_vms && !processes.is_empty() {
        panic!(
            "{label} requires a quiet host; found existing Firecracker \
             processes: {processes:?}. Stop them, or set {allow_env}=1 for a \
             non-closeable diagnostic run"
        );
    }
    (allow_other_firecracker_vms, processes)
}

pub(super) fn substrate_json(
    allow_other_firecracker_vms: bool,
    processes: &[FirecrackerProcess],
) -> serde_json::Value {
    serde_json::json!({
        "substrate_kind": "real-kvm",
        "preflight_required": true,
        "quiet_host_checked": true,
        "allow_other_firecracker_vms": allow_other_firecracker_vms,
        "preexisting_firecracker_processes": processes
            .iter()
            .map(FirecrackerProcess::to_json)
            .collect::<Vec<_>>(),
        "host_kernel_release": command_output("uname", &["-r"]),
        "dev_kvm_stat": command_output("stat", &["-c", "%A %U:%G %n", "/dev/kvm"]),
        "sudo_uid": command_output("id", &["-u"]),
    })
}

pub(super) fn record_post_run_firecracker_processes(
    substrate: &mut serde_json::Value,
    processes: &[FirecrackerProcess],
) {
    substrate["post_run_firecracker_processes"] = processes
        .iter()
        .map(FirecrackerProcess::to_json)
        .collect::<Vec<_>>()
        .into();
}

pub(super) fn record_preflight_artifacts(
    substrate: &mut serde_json::Value,
    discovery: &m80_preflight::Discovery,
) {
    substrate["preflight_artifacts"] = preflight_artifacts_json(discovery);
    substrate["firecracker_version"] =
        command_output(discovery.firecracker_bin.as_path(), &["--version"]).into();
}

fn preflight_artifacts_json(discovery: &m80_preflight::Discovery) -> serde_json::Value {
    serde_json::json!({
        "firecracker_bin": discovery.firecracker_bin,
        "firecracker_seccomp_filter": discovery.firecracker_seccomp_filter,
        "jailer_bin": discovery.jailer_bin,
        "jailer_harden_bin": discovery.jailer_harden_bin,
        "net_helper_bin": discovery.net_helper_bin,
        "kernel_image": discovery.kernel,
        "rootfs_image": discovery.rootfs,
        "kernel_image_sha256": sha256_file_hex(&discovery.kernel),
        "rootfs_image_sha256": sha256_file_hex(&discovery.rootfs),
        "kernel_kind": discovery.manifest.kernel_kind,
        "image_kind": discovery.manifest.image_kind,
        "rootfs_format": discovery.manifest.rootfs_format,
        "expected_firecracker_version": discovery.manifest.expected_firecracker_version,
    })
}

fn sha256_file_hex(path: &Path) -> String {
    let mut file = std::fs::File::open(path)
        .unwrap_or_else(|err| panic!("open {} for sha256: {err}", path.display()));
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)
        .unwrap_or_else(|err| panic!("hash {} for sha256: {err}", path.display()));
    hex::encode(hasher.finalize())
}

fn command_output(command: impl AsRef<std::ffi::OsStr>, args: &[&str]) -> String {
    match std::process::Command::new(command).args(args).output() {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }
        Ok(output) => format!("exit status {}", output.status),
        Err(err) => format!("error: {err}"),
    }
}

pub(super) fn git_worktree_dirty_excluding(paths: &[PathBuf]) -> bool {
    let repo = repo_root();
    let allowed = paths
        .iter()
        .map(|path| artifact_relative_path(path, &repo))
        .collect::<Vec<_>>();
    let output = std::process::Command::new("git")
        .args([
            "-C",
            &repo.display().to_string(),
            "status",
            "--porcelain=v1",
        ])
        .output();
    let Ok(output) = output else {
        return true;
    };
    if !output.status.success() {
        return true;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .any(|line| {
            let path = line
                .strip_prefix("?? ")
                .or_else(|| line.get(3..))
                .unwrap_or(line)
                .trim();
            !allowed.iter().any(|allowed| allowed == path)
        })
}

pub(super) fn git_head_commit() -> String {
    let repo = repo_root();
    let output = std::process::Command::new("git")
        .args(["-C", &repo.display().to_string(), "rev-parse", "HEAD"])
        .output()
        .expect("spawn git rev-parse HEAD");
    assert!(output.status.success(), "git rev-parse HEAD failed");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn artifact_relative_path(path: &Path, repo: &Path) -> String {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo.join(path)
    };
    absolute
        .strip_prefix(repo)
        .ok()
        .and_then(|path| path.to_str())
        .unwrap_or_default()
        .to_owned()
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root")
        .to_path_buf()
}

pub(super) fn firecracker_processes() -> Vec<FirecrackerProcess> {
    let proc = std::fs::read_dir("/proc").expect("read /proc");
    let mut processes = Vec::new();
    for entry in proc {
        let entry = entry.unwrap_or_else(|err| panic!("read /proc entry: {err}"));
        let file_name = entry.file_name();
        let Some(pid) = file_name.to_str().and_then(|raw| raw.parse::<u32>().ok()) else {
            continue;
        };
        let cmdline = match std::fs::read(entry.path().join("cmdline")) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let argv = cmdline
            .split(|byte| *byte == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect::<Vec<_>>();
        let Some(program) = argv.first() else {
            continue;
        };
        let basename = Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(program);
        if basename == "firecracker" {
            processes.push(FirecrackerProcess { pid, argv });
        }
    }
    processes.sort_by_key(|process| process.pid);
    processes
}

fn preexisting_firecracker_processes() -> Vec<FirecrackerProcess> {
    firecracker_processes()
}

fn env_bool(name: &str) -> bool {
    match std::env::var(name).as_deref() {
        Ok("1" | "true" | "TRUE" | "yes" | "YES") => true,
        Ok("0" | "false" | "FALSE" | "no" | "NO") | Err(_) => false,
        Ok(raw) => panic!("{name} must be boolean-like, got {raw:?}"),
    }
}
