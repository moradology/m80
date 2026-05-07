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

use m80_proto::{
    CancelRequest, CancelResponse, CancelStatus, Envelope, ExecRequest,
    PAYLOAD_KIND_CANCEL_RESPONSE,
};
use m80_vsock::{Channel, GUEST_PORT_DEFAULT};

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
            vm_id: Some("cancel-test".into()),
            workspace: None,
            network: m80_firecracker::NetworkPolicy::NoEgress,
            vcpu_count: Some(1),
            mem_size_mib: Some(512),
            boot_args: None,
            overlay_size_bytes: 256 * 1024 * 1024,
            idle_timeout: None,
            daemonize: false,
            request_id: None,
            preallocated_drive_slots: 0,
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
            program: "/bin/sleep".into(),
            args: vec!["60".into()],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(30_000),
            streaming: false,
        },
        "cancel-test-req-1".into(),
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
            program: "/bin/sleep".into(),
            args: vec!["60".into()],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        },
        "real-req".into(),
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
