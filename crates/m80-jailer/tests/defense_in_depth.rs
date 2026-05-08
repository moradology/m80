use std::path::{Path, PathBuf};

use m80_jailer::{JailerConfig, JailerSocket, Plan, ResourceLimits};
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::Pid;

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn attack_runner_echo_zero_negative_control_reports_breach() {
    let result = run_attack_in_jailer("echo_zero").expect("run attack");

    assert_eq!(
        result.exit_code,
        Some(0),
        "echo_zero must prove the harness sees a successful attack as a breach"
    );
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn attack_runner_unknown_attack_reports_blocked() {
    let result = run_attack_in_jailer("missing_attack").expect("run attack");

    assert_ne!(
        result.exit_code,
        Some(0),
        "unknown attack must exercise the harness blocked/nonzero path"
    );
}

struct AttackRun {
    exit_code: Option<i32>,
}

fn run_attack_in_jailer(name: &str) -> Result<AttackRun, Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let run_dir = temp.path().join(format!("attack-{name}"));
    std::fs::create_dir(&run_dir)?;
    let stdio_log = run_dir.join("attack-runner.log");
    let config = JailerConfig {
        jailer_bin: binary_from_env("M80_JAILER_BIN", "/usr/bin/jailer")?,
        jailer_harden_bin: Some(binary_from_env(
            "M80_JAILER_HARDEN_BIN",
            "/usr/bin/m80-jailer-harden",
        )?),
        firecracker_bin: attack_runner_bin()?,
        run_dir,
        uid: 3000,
        gid: 3000,
        bindings: Vec::new(),
        sockets: vec![JailerSocket::Firecracker],
        resource_limits: ResourceLimits::default(),
        new_pid_ns: false,
        daemonize: false,
        new_cgroup_ns: false,
        netns_path: None,
        stdio_log: Some(stdio_log),
    };
    let jail = Plan::compute(&config)?.materialize()?;
    let jailed = jail.launch(Path::new(name))?;
    let status = waitpid(Pid::from_raw(jailed.firecracker_pid as i32), None)?;
    Ok(AttackRun {
        exit_code: match status {
            WaitStatus::Exited(_, code) => Some(code),
            WaitStatus::Signaled(_, signal, _) => Some(128 + signal as i32),
            other => panic!("unexpected attack-runner wait status: {other:?}"),
        },
    })
}

fn binary_from_env(env: &str, default: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let raw = std::env::var(env).unwrap_or_else(|_| default.to_owned());
    Ok(std::fs::canonicalize(raw)?)
}

fn attack_runner_bin() -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Ok(path) = std::env::var("M80_ATTACK_RUNNER_BIN") {
        return Ok(std::fs::canonicalize(path)?);
    }
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate lives under workspace/crates");
    let path = workspace
        .join("target")
        .join("x86_64-unknown-linux-musl")
        .join("debug")
        .join("m80-attack-runner");
    Ok(std::fs::canonicalize(path)?)
}
