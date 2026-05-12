//! Real-KVM coverage for user-visible PTY interaction semantics.

mod common;

use std::sync::mpsc;

use m80_firecracker::{Backend, BackendConfig, CgroupMode, FcError, NetworkPolicy, SandboxConfig};
use m80_proto::{ExecStatus, PtyControlEvent, PtyRequest, PtySignal, PtySize};

use common::RunDirDumpGuard;

fn launch_vm() -> (m80_firecracker::RunningSandbox, std::path::PathBuf) {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let run_root = discovery.run_root.clone();
    let config = BackendConfig {
        discovery,
        max_concurrent_vms: 1,
        run_root,
        jail_uid: 3000,
        jail_gid: 3000,
        cgroup_mode: CgroupMode::Disabled,
    };
    let backend = std::sync::Arc::new(Backend::new(config).expect("Backend::new"));
    let sandbox = backend
        .admit(SandboxConfig {
            vm_id: Some(common::unique_vm_id("pty-e2e")),
            workspace: None,
            network: NetworkPolicy::NoEgress,
            vcpu_count: Some(1),
            mem_size_mib: Some(512),
            boot_args: None,
            overlay_size_bytes: 256 * 1024 * 1024,
            idle_timeout: None,
            daemonize: false,
            request_id: None,
            preallocated_drive_slots: 0,
            one_shot: false,
        })
        .expect("admit");
    let running = sandbox.launch().expect("launch");
    let run_dir = running.run_dir().to_owned();
    (running, run_dir)
}

fn pty_size(rows: u16, cols: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: None,
        pixel_height: None,
    }
}

fn pty_request(shell: &str, size: PtySize) -> PtyRequest {
    PtyRequest {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), shell.into()],
        cwd: None,
        env: None,
        timeout_ms: Some(10_000),
        size,
    }
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn pty_eof_on_stdin_completes_read() {
    let (mut running, run_dir) = launch_vm();
    let _dump_guard = RunDirDumpGuard::new(run_dir);
    let (tx, rx) = mpsc::channel();
    tx.send(m80_firecracker::PtyHostEvent::Input(
        b"eof-through-pty\n\x04".to_vec(),
    ))
    .expect("queue pty input");
    drop(tx);

    let mut output = Vec::new();
    let exit = running
        .exec_pty(pty_request("cat", pty_size(24, 80)), rx, |chunk| {
            output.extend_from_slice(&chunk.bytes);
            Ok(())
        })
        .expect("exec pty eof");

    assert_eq!(exit.status, ExecStatus::Completed);
    assert_eq!(exit.exit_code, Some(0));
    assert!(
        String::from_utf8_lossy(&output).contains("eof-through-pty"),
        "PTY EOF should let cat complete after seeing host input, got {:?}",
        String::from_utf8_lossy(&output)
    );

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn pty_sigint_during_read_returns_exit_130() {
    let (mut running, run_dir) = launch_vm();
    let _dump_guard = RunDirDumpGuard::new(run_dir);
    let (tx, rx) = mpsc::channel();
    let mut sent_interrupt = false;
    let mut output = Vec::new();

    let exit = running
        .exec_pty(
            pty_request(
                "trap 'exit 130' INT; printf M80_SIGINT_READY; read line",
                pty_size(24, 80),
            ),
            rx,
            |chunk| {
                output.extend_from_slice(&chunk.bytes);
                if !sent_interrupt
                    && output
                        .windows(b"M80_SIGINT_READY".len())
                        .any(|w| w == b"M80_SIGINT_READY")
                {
                    tx.send(m80_firecracker::PtyHostEvent::Control(
                        PtyControlEvent::Signal {
                            signal: PtySignal::Interrupt,
                        },
                    ))
                    .map_err(|_| FcError::InvalidState {
                        expected: "pty interrupt receiver alive",
                        actual: "closed",
                    })?;
                    sent_interrupt = true;
                }
                Ok(())
            },
        )
        .expect("exec pty sigint");

    assert!(sent_interrupt, "test did not observe readiness marker");
    assert_eq!(exit.status, ExecStatus::Completed);
    assert_eq!(exit.exit_code, Some(130));

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn pty_sigwinch_propagates_dimensions() {
    let (mut running, run_dir) = launch_vm();
    let _dump_guard = RunDirDumpGuard::new(run_dir);
    let (tx, rx) = mpsc::channel();
    tx.send(m80_firecracker::PtyHostEvent::Resize(pty_size(44, 120)))
        .expect("queue pty resize");
    drop(tx);

    let mut output = Vec::new();
    let exit = running
        .exec_pty(
            pty_request("sleep 0.2; stty size", pty_size(24, 80)),
            rx,
            |chunk| {
                output.extend_from_slice(&chunk.bytes);
                Ok(())
            },
        )
        .expect("exec pty resize");

    assert_eq!(exit.status, ExecStatus::Completed);
    assert_eq!(exit.exit_code, Some(0));
    assert!(
        String::from_utf8_lossy(&output)
            .replace('\r', "")
            .lines()
            .any(|line| line.trim() == "44 120"),
        "PTY resize should be visible to stty size, got {:?}",
        String::from_utf8_lossy(&output)
    );

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}
