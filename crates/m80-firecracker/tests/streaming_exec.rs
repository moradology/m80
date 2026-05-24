//! Real-KVM streaming exec smoke tests.

mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use common::RunDirDumpGuard;
use m80_firecracker::{
    Backend, BackendConfig, CgroupMode, DisconnectCause, ExecChunk, FcError, SandboxConfig,
    WireProtocolError,
};
use m80_proto::{ExecRequest, ExecStatus};

fn backend() -> (Arc<Backend>, std::path::PathBuf) {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let run_root = discovery.run_root.clone();
    let config = BackendConfig::builder(discovery)
        .max_concurrent_vms(1)
        .run_root(run_root.clone())
        .jail_uid(jail_id_from_env("M80_JAIL_UID", 3000))
        .jail_gid(jail_id_from_env("M80_JAIL_GID", 3000))
        .cgroup_mode(CgroupMode::Disabled)
        .build();
    (
        Arc::new(Backend::new(config).expect("Backend::new")),
        run_root,
    )
}

fn jail_id_from_env(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn sandbox_config(vm_id: &str) -> SandboxConfig {
    SandboxConfig {
        vcpu_count: Some(1),
        mem_size_mib: Some(512),
        cpuset_cpus: None,
        cpu_template: None,
        fc_log_level: None,
        ..common::sandbox_config_with_id(vm_id)
    }
}

fn mixed_output_request() -> ExecRequest {
    ExecRequest {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), "echo a; echo b >&2; exit 7".into()],
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(5_000),
        streaming: false,
    }
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn exec_and_exec_streaming_report_equivalent_output() {
    let (backend, run_root) = backend();
    let sandbox = backend
        .admit(sandbox_config("streaming-equivalence"))
        .expect("admit");
    let mut running = sandbox.launch().expect("launch");
    let _dump_guard = RunDirDumpGuard::new(run_root.join("streaming-equivalence"));

    let buffered = running.exec(mixed_output_request()).expect("buffered exec");

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit = running
        .exec_streaming(mixed_output_request(), |chunk| match chunk {
            ExecChunk::Stdout { bytes, .. } => {
                stdout.extend(bytes);
                Ok(())
            }
            ExecChunk::Stderr { bytes, .. } => {
                stderr.extend(bytes);
                Ok(())
            }
        })
        .expect("streaming exec");

    assert_eq!(buffered.status, ExecStatus::Completed);
    assert_eq!(buffered.exit_code, Some(7));
    assert_eq!(exit.status, ExecStatus::Completed);
    assert_eq!(exit.exit_code, Some(7));
    assert_eq!(buffered.stdout, stdout);
    assert_eq!(buffered.stderr, stderr);

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn exec_with_no_output_longer_than_bridge_timeout_completes() {
    let (backend, run_root) = backend();
    let vm_id = "longidle";
    let sandbox = backend.admit(sandbox_config(vm_id)).expect("admit");
    let mut running = sandbox.launch().expect("launch");
    let _dump_guard = RunDirDumpGuard::new(run_root.join(vm_id));

    let started = Instant::now();
    let resp = running
        .exec(ExecRequest {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "sleep 30; printf bridge-timeout-ok".into()],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(45_000),
            streaming: false,
        })
        .expect("idle exec must wait for guest terminal frame, not bridge I/O timeout");

    assert!(
        started.elapsed() >= Duration::from_secs(30),
        "test must stay silent longer than the old 5s bridge timeout"
    );
    assert_eq!(resp.status, ExecStatus::Completed);
    assert_eq!(resp.exit_code, Some(0));
    assert_eq!(String::from_utf8_lossy(&resp.stdout), "bridge-timeout-ok");

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn dropped_streaming_caller_releases_guestd_for_next_exec() {
    let (backend, run_root) = backend();
    let sandbox = backend
        .admit(sandbox_config("streaming-drop-cancel"))
        .expect("admit");
    let mut running = sandbox.launch().expect("launch");
    let _dump_guard = RunDirDumpGuard::new(run_root.join("streaming-drop-cancel"));

    let result = catch_unwind(AssertUnwindSafe(|| {
        running
            .exec_streaming(
                ExecRequest {
                    program: "yes".into(),
                    args: vec![],
                    cwd: None,
                    env: None,
                    stdin: None,
                    timeout_ms: Some(30_000),
                    streaming: false,
                },
                |_chunk| -> Result<(), m80_firecracker::FcError> {
                    panic!("drop streaming caller after first chunk")
                },
            )
            .expect("streaming yes should start");
    }));
    assert!(
        result.is_err(),
        "callback panic should abort the streaming call"
    );

    std::thread::sleep(Duration::from_millis(250));
    let resp = running
        .exec(ExecRequest {
            program: "/bin/true".into(),
            args: vec![],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        })
        .expect("guestd should accept a new exec after dropped streaming caller");
    assert_eq!(resp.status, ExecStatus::Completed);
    assert_eq!(resp.exit_code, Some(0));

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn cancellable_streaming_exec_kills_shell_grandchild_and_allows_next_exec() {
    let (backend, run_root) = backend();
    let sandbox = backend
        .admit(sandbox_config("streaming-cancel-grandchild"))
        .expect("admit");
    let mut running = sandbox.launch().expect("launch");
    let _dump_guard = RunDirDumpGuard::new(run_root.join("streaming-cancel-grandchild"));

    let (cancel_tx, cancel_rx) = mpsc::channel();
    let mut stdout = Vec::new();
    let exit = running
        .exec_streaming_with_cancel(
            ExecRequest {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), "printf ready; sleep 60 & wait".into()],
                cwd: None,
                env: None,
                stdin: None,
                timeout_ms: Some(30_000),
                streaming: false,
            },
            cancel_rx,
            |chunk| {
                if let ExecChunk::Stdout { bytes, .. } = chunk {
                    stdout.extend(bytes);
                    if stdout == b"ready" {
                        let _ = cancel_tx.send(());
                    }
                }
                Ok(())
            },
        )
        .expect("streaming exec should cancel");

    assert_eq!(exit.status, ExecStatus::Cancelled);
    assert_eq!(stdout, b"ready");

    let resp = running
        .exec(ExecRequest {
            program: "/bin/true".into(),
            args: vec![],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        })
        .expect("guestd should accept a new exec after group cancellation");
    assert_eq!(resp.status, ExecStatus::Completed);
    assert_eq!(resp.exit_code, Some(0));

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn cancellable_large_stdout_stream_kills_writer_and_allows_next_exec() {
    let (backend, run_root) = backend();
    let vm_id = "scls";
    let sandbox = backend.admit(sandbox_config(vm_id)).expect("admit");
    let mut running = sandbox.launch().expect("launch");
    let _dump_guard = RunDirDumpGuard::new(run_root.join(vm_id));

    let (cancel_tx, cancel_rx) = mpsc::channel();
    let mut stdout_total = 0usize;
    let mut cancel_sent = false;
    let exit = running
        .exec_streaming_with_cancel(
            ExecRequest {
                program: "/bin/sh".into(),
                args: vec![
                    "-c".into(),
                    "while :; do printf 'm80-large-stdout-cancel\\n'; done".into(),
                ],
                cwd: None,
                env: None,
                stdin: None,
                timeout_ms: Some(30_000),
                streaming: false,
            },
            cancel_rx,
            |chunk| {
                if let ExecChunk::Stdout { bytes, .. } = chunk {
                    stdout_total = stdout_total.saturating_add(bytes.len());
                    if stdout_total >= 64 * 1024 && !cancel_sent {
                        let _ = cancel_tx.send(());
                        cancel_sent = true;
                    }
                }
                Ok(())
            },
        )
        .expect("large stdout streaming exec should cancel");

    assert!(cancel_sent, "test never observed enough stdout to cancel");
    assert_eq!(exit.status, ExecStatus::Cancelled);
    assert!(
        stdout_total >= 64 * 1024,
        "expected at least 64 KiB before cancellation, got {stdout_total}"
    );

    let resp = running
        .exec(ExecRequest {
            program: "/bin/true".into(),
            args: vec![],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        })
        .expect("guestd should accept a new exec after large stdout cancellation");
    assert_eq!(resp.status, ExecStatus::Completed);
    assert_eq!(resp.exit_code, Some(0));

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn disconnect_mid_streaming_exec_maps_to_disconnect_before_terminal() {
    let (backend, run_root) = backend();
    let vm_id = "sdmx";
    let sandbox = backend.admit(sandbox_config(vm_id)).expect("admit");
    let mut running = sandbox.launch().expect("launch");
    let _dump_guard = RunDirDumpGuard::new(run_root.join(vm_id));
    let firecracker_pid = firecracker_pid(running.run_dir());

    let mut stdout_total = 0usize;
    let mut killed_firecracker = false;
    let err = running
        .exec_streaming(
            ExecRequest {
                program: "/bin/sh".into(),
                args: vec![
                    "-c".into(),
                    "while :; do printf 'm80-before-disconnect\\n'; done".into(),
                ],
                cwd: None,
                env: None,
                stdin: None,
                timeout_ms: Some(30_000),
                streaming: false,
            },
            |chunk| {
                if let ExecChunk::Stdout { bytes, .. } = chunk {
                    stdout_total = stdout_total.saturating_add(bytes.len());
                    if stdout_total >= 64 * 1024 && !killed_firecracker {
                        kill_process(firecracker_pid);
                        killed_firecracker = true;
                    }
                }
                Ok(())
            },
        )
        .expect_err("guestd disconnect before exec_exit should be typed");

    assert!(stdout_total > 0, "test did not observe stdout before drop");
    assert!(
        killed_firecracker,
        "test never reached the host-side kill threshold"
    );
    assert!(
        matches!(
            err,
            FcError::Protocol(WireProtocolError::DisconnectBeforeTerminal {
                context: "streaming exec",
                cause: DisconnectCause::FcProcessDead
            })
        ),
        "expected DisconnectBeforeTerminal(streaming exec, FcProcessDead), got {err:?}"
    );

    let stopped = running.stop().expect("stop after guest disconnect");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn exec_on_dead_firecracker_fails_fast_without_open_retry() {
    let (backend, run_root) = backend();
    let vm_id = "deadx";
    let sandbox = backend.admit(sandbox_config(vm_id)).expect("admit");
    let mut running = sandbox.launch().expect("launch");
    let _dump_guard = RunDirDumpGuard::new(run_root.join(vm_id));
    let firecracker_pid = firecracker_pid(running.run_dir());

    kill_process(firecracker_pid);
    wait_for_process_exit(firecracker_pid);

    let started = Instant::now();
    let err = running
        .exec(ExecRequest {
            program: "/bin/true".into(),
            args: Vec::new(),
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        })
        .expect_err("dead firecracker should fail before vsock retry loop");
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_millis(500),
        "dead sandbox exec took {elapsed:?}, expected liveness guard before retry loop"
    );
    assert!(
        matches!(
            err,
            FcError::SandboxDead {
                vm_id: ref observed_vm_id,
                firecracker_pid: observed_pid
            } if observed_vm_id == vm_id && observed_pid == firecracker_pid
        ),
        "expected SandboxDead for {vm_id}/{firecracker_pid}, got {err:?}"
    );

    running
        .force_kill()
        .expect("force kill already-dead sandbox")
        .delete()
        .expect("delete");
}

fn firecracker_pid(run_dir: &std::path::Path) -> u32 {
    let state_path = run_dir.join("jailer-state.json");
    let state: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&state_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", state_path.display())),
    )
    .expect("jailer-state.json parses");
    state["firecracker_pid"]
        .as_u64()
        .unwrap_or_else(|| panic!("firecracker_pid missing from {}", state_path.display()))
        as u32
}

fn kill_process(pid: u32) {
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pid as i32),
        nix::sys::signal::Signal::SIGKILL,
    )
    .unwrap_or_else(|e| panic!("kill firecracker pid {pid}: {e}"));
}

fn wait_for_process_exit(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if !process_is_live(pid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("firecracker pid {pid} still exists after SIGKILL");
}

fn process_is_live(pid: u32) -> bool {
    let proc_dir = std::path::PathBuf::from(format!("/proc/{pid}"));
    match std::fs::read_to_string(proc_dir.join("stat")) {
        Ok(stat) => !matches!(proc_stat_state(&stat), Some('Z' | 'X')),
        Err(_) => proc_dir.exists(),
    }
}

fn proc_stat_state(stat: &str) -> Option<char> {
    let (_comm, after_comm) = stat.rsplit_once(") ")?;
    after_comm.chars().next()
}
