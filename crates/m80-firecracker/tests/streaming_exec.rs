//! Real-KVM streaming exec smoke tests.

mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use common::RunDirDumpGuard;
use m80_firecracker::{
    Backend, BackendConfig, CgroupMode, ExecChunk, ExecRequest, ExecStatus, NetworkPolicy,
    SandboxConfig,
};

fn backend() -> (Arc<Backend>, std::path::PathBuf) {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let run_root = discovery.run_root.clone();
    let config = BackendConfig {
        discovery,
        max_concurrent_vms: 1,
        run_root: run_root.clone(),
        jail_uid: 3000,
        jail_gid: 3000,
        cgroup_mode: CgroupMode::Disabled,
    };
    (
        Arc::new(Backend::new(config).expect("Backend::new")),
        run_root,
    )
}

fn sandbox_config(vm_id: &str) -> SandboxConfig {
    SandboxConfig {
        vm_id: Some(vm_id.into()),
        workspace: None,
        network: NetworkPolicy::NoEgress,
        vcpu_count: Some(1),
        mem_size_mib: Some(512),
        boot_args: None,
        overlay_size_bytes: 512 * 1024 * 1024,
        idle_timeout: None,
        request_id: None,
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
