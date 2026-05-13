//! Wire-level cancellation contract: cancel envelope, guestd handler, CancelResponse.
//!
//! All tests are `#[ignore]` and require a KVM-capable host with Firecracker +
//! jailer binaries and a built m80 guest image — run with:
//!
//! ```text
//! sudo cargo test -p m80-firecracker -- --ignored cancel_
//! ```
//!
//! ## Wire strategy
//!
//! `RunningSandbox::exec` is a blocking call that holds the vsock channel for
//! the duration of the exec. To inject a cancel mid-flight from the same
//! process the test opens a raw `Channel` (bypassing `RunningSandbox`) and
//! sends both the `exec_request` and the `cancel_request` frames before
//! reading any response. The kernel buffers both frames; guestd reads the
//! exec frame, spawns the process, polls the buffer, finds the cancel, SIGKILLs,
//! and replies with `CancelResponse`. This faithfully exercises the guestd cancel
//! handler without requiring two threads or a Drop-guard (both Wave-4 concerns).
//!
//! The UDS path is derived from the jailer layout:
//! `{run_dir}/{fc_binary_basename}/{vm_id}/root/vsock.sock`.

mod common;

use std::path::PathBuf;
use std::time::Instant;

use m80_proto::GUEST_PORT_DEFAULT;
use m80_proto::{
    CancelRequest, CancelResponse, CancelStatus, Envelope, ExecRequest, ExecResponse, ExecStatus,
    FileError, FileWriteBeginRequest, FileWriteBeginResponse, FileWriteChunkRequest,
    FileWriteChunkResponse, PAYLOAD_KIND_CANCEL_RESPONSE,
};
use m80_vsock::Channel;

use common::RunDirDumpGuard;

/// Derive the vsock UDS path from the jailer layout conventions.
///
/// Jailer places the chroot at:
///   `{chroot_base_dir}/{fc_binary_basename}/{id}/root/`
/// where `chroot_base_dir` is `run_dir` and `id` is `vm_id`.
/// The Firecracker vsock device creates its UDS at
///   `{jail_root}/vsock.sock`.
fn vsock_uds(run_dir: &std::path::Path, firecracker_bin: &std::path::Path, vm_id: &str) -> PathBuf {
    let fc_basename = firecracker_bin
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("firecracker"));
    run_dir
        .join(fc_basename)
        .join(vm_id)
        .join("root")
        .join("vsock.sock")
}

fn launch_vm(
    discovery: &m80_preflight::Discovery,
    run_root: &std::path::Path,
) -> (m80_firecracker::RunningSandbox, PathBuf) {
    let config = m80_firecracker::BackendConfig {
        discovery: discovery.clone(),
        max_concurrent_vms: 1,
        run_root: run_root.to_owned(),
        jail_uid: 3000,
        jail_gid: 3000,
        cgroup_mode: m80_firecracker::CgroupMode::Disabled,
    };
    let backend = std::sync::Arc::new(m80_firecracker::Backend::new(config).expect("Backend::new"));
    let sandbox = backend
        .admit(m80_firecracker::SandboxConfig {
            vm_id: Some(common::unique_vm_id("cancel-test")),
            workspace: None,
            network: m80_firecracker::NetworkPolicy::NoEgress,
            vcpu_count: Some(1),
            mem_size_mib: Some(512),
            cpuset_cpus: None,
            cpu_template: None,
            drive_cache_type: None,
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

/// Open a raw vsock channel to the running VM, bypassing `RunningSandbox::exec`
/// so we can inject two frames before reading the response.
fn open_raw_channel(
    run_dir: &std::path::Path,
    firecracker_bin: &std::path::Path,
    vm_id: &str,
) -> Channel {
    let uds = vsock_uds(run_dir, firecracker_bin, vm_id);
    Channel::open_uds_only(&uds, GUEST_PORT_DEFAULT)
        .expect("failed to open raw vsock channel for cancel test")
}

// ── Scenario 1: cancel kills a running process ────────────────────────────────

/// Boot VM → send `sleep 60` exec + cancel on the same channel → assert
/// `CancelResponse { status: Cancelled }` arrives, total elapsed << 60 s.
#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn cancel_kills_running_process() {
    let discovery = m80_preflight::run().expect("preflight");
    let run_root = discovery.run_root.clone();
    let (running, run_dir) = launch_vm(&discovery, &run_root);
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());

    let vm_id = running.vm_id().to_owned();
    let mut channel = open_raw_channel(&run_dir, &discovery.firecracker_bin, &vm_id);

    // Send exec_request (sleep 60).
    let exec_env = Envelope::with_request_id(
        ExecRequest {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "sleep 60".into()],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(30_000),
            streaming: false,
        },
        "cancel-test-req-1".to_owned(),
    );
    channel.send(&exec_env).expect("send exec_request");

    // Immediately send cancel_request on the same channel.
    let cancel_env = Envelope::new(CancelRequest {
        request_id: "cancel-test-req-1".into(),
    });
    channel.send(&cancel_env).expect("send cancel_request");

    // Read the CancelResponse response.
    let start = Instant::now();
    let ack_env: Envelope<CancelResponse> = channel.recv().expect("recv CancelResponse");
    let elapsed = start.elapsed();

    assert_eq!(
        ack_env.kind, PAYLOAD_KIND_CANCEL_RESPONSE,
        "expected cancel_ack envelope"
    );
    assert_eq!(ack_env.payload.request_id, "cancel-test-req-1");
    assert_eq!(ack_env.payload.status, CancelStatus::Cancelled);
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "cancel should complete in <10 s, elapsed={elapsed:?}"
    );

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn two_concurrent_cancels_idempotent() {
    let discovery = m80_preflight::run().expect("preflight");
    let run_root = discovery.run_root.clone();
    let (running, run_dir) = launch_vm(&discovery, &run_root);
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());

    let vm_id = running.vm_id().to_owned();
    let mut channel = open_raw_channel(&run_dir, &discovery.firecracker_bin, &vm_id);
    let request_id = "double-cancel-req";

    channel
        .send(&Envelope::with_request_id(
            ExecRequest {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), "sleep 60".into()],
                cwd: None,
                env: None,
                stdin: None,
                timeout_ms: Some(30_000),
                streaming: true,
            },
            request_id.to_owned(),
        ))
        .expect("send exec_request");
    channel
        .send(&Envelope::new(CancelRequest {
            request_id: request_id.to_owned(),
        }))
        .expect("send first cancel_request");
    channel
        .send(&Envelope::new(CancelRequest {
            request_id: request_id.to_owned(),
        }))
        .expect("send second cancel_request");

    let first: Envelope<CancelResponse> = channel.recv().expect("recv first cancel ack");
    let second: Envelope<CancelResponse> = channel.recv().expect("recv second cancel ack");

    assert_eq!(first.kind, PAYLOAD_KIND_CANCEL_RESPONSE);
    assert_eq!(first.payload.request_id, request_id);
    assert_eq!(first.payload.status, CancelStatus::Cancelled);
    assert_eq!(second.kind, PAYLOAD_KIND_CANCEL_RESPONSE);
    assert_eq!(second.payload.request_id, request_id);
    assert_eq!(second.payload.status, CancelStatus::AlreadyExited);

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn cancel_then_disconnect_handles_lost_ack() {
    let discovery = m80_preflight::run().expect("preflight");
    let run_root = discovery.run_root.clone();
    let (mut running, run_dir) = launch_vm(&discovery, &run_root);
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());

    let vm_id = running.vm_id().to_owned();
    let mut channel = open_raw_channel(&run_dir, &discovery.firecracker_bin, &vm_id);
    let request_id = "cancel-lost-ack-req";
    let pid_path = "/tmp/cancel-lost-ack.pid";

    channel
        .send(&Envelope::with_request_id(
            ExecRequest {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), format!("echo $$ > {pid_path}; sleep 60")],
                cwd: None,
                env: None,
                stdin: None,
                timeout_ms: Some(30_000),
                streaming: false,
            },
            request_id.to_owned(),
        ))
        .expect("send exec_request");
    std::thread::sleep(std::time::Duration::from_millis(250));

    channel
        .send(&Envelope::new(CancelRequest {
            request_id: request_id.to_owned(),
        }))
        .expect("send cancel frame before disconnect");
    drop(channel);

    std::thread::sleep(std::time::Duration::from_millis(500));
    let probe = running
        .exec(ExecRequest {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                format!(
                    "pid=$(cat {pid_path}) || exit 43; \
                     if [ -d /proc/$pid ]; then echo leaked:$pid >&2; exit 42; fi; \
                     printf cancelled"
                ),
            ],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        })
        .expect("guestd should accept new exec after lost cancel ack");
    assert_eq!(probe.status, ExecStatus::Completed);
    assert_eq!(probe.exit_code, Some(0), "stderr={:?}", probe.stderr);
    assert_eq!(probe.stdout, b"cancelled");

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn cancel_frame_mid_chunked_upload_leaves_no_partial_files() {
    let discovery = m80_preflight::run().expect("preflight");
    let run_root = discovery.run_root.clone();
    let (mut running, run_dir) = launch_vm(&discovery, &run_root);
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());

    let vm_id = running.vm_id().to_owned();
    let mut channel = open_raw_channel(&run_dir, &discovery.firecracker_bin, &vm_id);
    let final_path = "/tmp/cancelled-upload.bin";
    let request_id = "cancel-upload-req";

    channel
        .send(&Envelope::with_request_id(
            FileWriteBeginRequest {
                path: final_path.to_owned(),
                mode: Some(0o600),
            },
            request_id.to_owned(),
        ))
        .expect("send upload begin");
    let begin: Envelope<FileWriteBeginResponse> = channel.recv().expect("recv upload begin");
    let upload_id = begin.payload.upload_id.expect("upload id");
    assert_eq!(begin.payload.error, None);

    for seq in 0..5 {
        channel
            .send(&Envelope::with_request_id(
                FileWriteChunkRequest {
                    upload_id: upload_id.clone(),
                    seq,
                    bytes: vec![b'x'; 1024 * 1024],
                },
                request_id.to_owned(),
            ))
            .expect("send upload chunk");
        let chunk: Envelope<FileWriteChunkResponse> = channel.recv().expect("recv upload chunk");
        assert_eq!(chunk.payload.error, None);
        assert_eq!(chunk.payload.bytes_written, 1024 * 1024);
    }

    channel
        .send(&Envelope::new(CancelRequest {
            request_id: request_id.to_owned(),
        }))
        .expect("send cancel frame during open upload");
    let close = channel.recv_raw();
    assert!(
        close.is_err(),
        "file-op cancel frame should close the upload channel without a response"
    );

    assert_file_not_found(&mut running, final_path);
    assert_file_not_found(
        &mut running,
        &format!("{final_path}.m80-upload.{upload_id}"),
    );

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

fn assert_file_not_found(running: &mut m80_firecracker::RunningSandbox, path: &str) {
    let err = running
        .stat_file(path)
        .expect_err("path should not exist after cancelled upload");
    assert!(
        matches!(err, m80_firecracker::FcError::FileOp(FileError::NotFound)),
        "expected FileError::NotFound for {path}, got {err:?}"
    );
}

// ── Scenario 2: cancel after process already exited ───────────────────────────

/// Boot VM → exec `/bin/true` → wait for it to complete → send cancel →
/// assert `CancelResponse { status: AlreadyExited }`.
#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn cancel_after_exit_returns_already_exited() {
    let discovery = m80_preflight::run().expect("preflight");
    let run_root = discovery.run_root.clone();
    let (mut running, run_dir) = launch_vm(&discovery, &run_root);
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());

    // Run /bin/true normally; exec blocks until it completes.
    let response = running
        .exec(ExecRequest {
            program: "/bin/true".into(),
            args: vec![],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        })
        .expect("exec /bin/true");
    assert_eq!(response.status, m80_proto::ExecStatus::Completed);

    // Send cancel on a fresh channel — no exec is in flight.
    let vm_id = running.vm_id().to_owned();
    let mut channel = open_raw_channel(&run_dir, &discovery.firecracker_bin, &vm_id);
    let cancel_env = Envelope::new(CancelRequest {
        request_id: "cancel-test-req-2".into(),
    });
    channel.send(&cancel_env).expect("send cancel_request");

    let ack_env: Envelope<CancelResponse> = channel.recv().expect("recv CancelResponse");
    assert_eq!(ack_env.kind, PAYLOAD_KIND_CANCEL_RESPONSE);
    assert_eq!(ack_env.payload.request_id, "cancel-test-req-2");
    assert_eq!(ack_env.payload.status, CancelStatus::AlreadyExited);

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

// ── Scenario 3: wrong request_id returns AlreadyExited ────────────────────────

/// Boot VM → send `sleep 60` exec + cancel with BOGUS request_id on same
/// channel → assert `CancelResponse { status: AlreadyExited }` (not Cancelled).
#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn wrong_request_id_returns_already_exited() {
    let discovery = m80_preflight::run().expect("preflight");
    let run_root = discovery.run_root.clone();
    let (running, run_dir) = launch_vm(&discovery, &run_root);
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());

    let vm_id = running.vm_id().to_owned();
    let mut channel = open_raw_channel(&run_dir, &discovery.firecracker_bin, &vm_id);

    // Send exec_request (sleep 60, with request_id "real-req").
    let exec_env = Envelope::with_request_id(
        ExecRequest {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "sleep 60".into()],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        },
        "real-req".to_owned(),
    );
    channel.send(&exec_env).expect("send exec_request");

    // Send cancel with a BOGUS request_id.
    let cancel_env = Envelope::new(CancelRequest {
        request_id: "bogus-req-id".into(),
    });
    channel.send(&cancel_env).expect("send cancel_request");

    // The first response should be the CancelResponse with AlreadyExited (wrong ID).
    let ack_env: Envelope<CancelResponse> = channel.recv().expect("recv CancelResponse");
    assert_eq!(ack_env.kind, PAYLOAD_KIND_CANCEL_RESPONSE);
    assert_eq!(ack_env.payload.request_id, "bogus-req-id");
    assert_eq!(ack_env.payload.status, CancelStatus::AlreadyExited);

    // The exec (sleep 60) will be killed by the 5 s timeout.
    // We don't need to read the ExecResponse; just stop the VM.
    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

/// Boot VM -> send `sleep 2; echo done` on channel A -> send matching cancel
/// on channel B -> assert channel B reports no in-flight exec and channel A
/// finishes normally. Cancellation is same-connection-only, not global by
/// request id.
#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn cancel_on_separate_connection_handled_safely() {
    let discovery = m80_preflight::run().expect("preflight");
    let run_root = discovery.run_root.clone();
    let (running, run_dir) = launch_vm(&discovery, &run_root);
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());

    let vm_id = running.vm_id().to_owned();
    let mut exec_channel = open_raw_channel(&run_dir, &discovery.firecracker_bin, &vm_id);
    let mut cancel_channel = open_raw_channel(&run_dir, &discovery.firecracker_bin, &vm_id);

    let request_id = "cancel-cross-connection";
    exec_channel
        .send(&Envelope::with_request_id(
            ExecRequest {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), "sleep 2; printf cross-connection-ok".into()],
                cwd: None,
                env: None,
                stdin: None,
                timeout_ms: Some(10_000),
                streaming: false,
            },
            request_id.to_owned(),
        ))
        .expect("send exec_request on channel A");

    cancel_channel
        .send(&Envelope::new(CancelRequest {
            request_id: request_id.to_owned(),
        }))
        .expect("send cancel_request on channel B");

    let ack_env: Envelope<CancelResponse> = cancel_channel
        .recv()
        .expect("recv cross-channel cancel ack");
    assert_eq!(ack_env.kind, PAYLOAD_KIND_CANCEL_RESPONSE);
    assert_eq!(ack_env.payload.request_id, request_id);
    assert_eq!(ack_env.payload.status, CancelStatus::AlreadyExited);

    let exec_env: Envelope<ExecResponse> = exec_channel
        .recv()
        .expect("recv original exec response after cross-channel cancel");
    assert_eq!(exec_env.request_id.as_deref(), Some(request_id));
    assert_eq!(exec_env.payload.status, ExecStatus::Completed);
    assert_eq!(exec_env.payload.exit_code, Some(0));
    assert_eq!(exec_env.payload.stdout, b"cross-connection-ok");

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}
