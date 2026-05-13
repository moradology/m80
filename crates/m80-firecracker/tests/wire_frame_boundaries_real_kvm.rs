//! Real-KVM wire frame size boundaries over Firecracker's vsock UDS.

mod common;

use std::cmp::Ordering;
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use m80_proto::GUEST_PORT_DEFAULT;
use m80_proto::{
    encode_raw_envelope, read_frame, read_raw_frame, write_frame, Envelope, ExecExit, ExecRequest,
    ExecResponse, ExecStatus, FileWriteRequest, FileWriteResponse, PingRequest, PongResponse,
    ProtoError, RawEnvelope, MAX_FRAME_BYTES, PAYLOAD_KIND_EXEC_EXIT, PAYLOAD_KIND_EXEC_STDOUT,
    PROTOCOL_VERSION,
};
use m80_vsock::Channel;

use common::RunDirDumpGuard;

fn vsock_uds(run_dir: &Path, firecracker_bin: &Path, vm_id: &str) -> PathBuf {
    let fc_basename = firecracker_bin
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("firecracker"));
    run_dir
        .join(fc_basename)
        .join(vm_id)
        .join("root")
        .join("vsock.sock")
}

fn launch_vm(discovery: &m80_preflight::Discovery) -> (m80_firecracker::RunningSandbox, PathBuf) {
    let config = m80_firecracker::BackendConfig {
        discovery: discovery.clone(),
        max_concurrent_vms: 1,
        run_root: discovery.run_root.clone(),
        jail_uid: 3000,
        jail_gid: 3000,
        cgroup_mode: m80_firecracker::CgroupMode::Disabled,
    };
    let backend = std::sync::Arc::new(m80_firecracker::Backend::new(config).expect("Backend::new"));
    let sandbox = backend
        .admit(m80_firecracker::SandboxConfig {
            vm_id: Some("wire-frame-boundary-test".into()),
            workspace: None,
            network: m80_firecracker::NetworkPolicy::NoEgress,
            vcpu_count: Some(1),
            mem_size_mib: Some(512),
            cpu_template: None,
            drive_cache_type: None,
            boot_args: None,
            overlay_size_bytes: 512 * 1024 * 1024,
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

fn open_channel(run_dir: &Path, firecracker_bin: &Path, vm_id: &str) -> Channel {
    let uds = vsock_uds(run_dir, firecracker_bin, vm_id);
    Channel::open_uds_only(&uds, GUEST_PORT_DEFAULT).expect("open vsock channel")
}

fn open_raw_stream(run_dir: &Path, firecracker_bin: &Path, vm_id: &str) -> BufReader<UnixStream> {
    let uds = vsock_uds(run_dir, firecracker_bin, vm_id);
    let stream = UnixStream::connect(&uds).expect("connect raw vsock uds");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set read timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .expect("set write timeout");

    let mut reader = BufReader::new(stream);
    writeln!(reader.get_mut(), "CONNECT {GUEST_PORT_DEFAULT}").expect("write CONNECT");
    let mut line = String::new();
    reader.read_line(&mut line).expect("read CONNECT response");
    assert!(
        line.starts_with("OK "),
        "unexpected vsock response: {line:?}"
    );
    reader
}

fn encoded_body_len<T>(envelope: &Envelope<T>) -> usize
where
    T: m80_proto::Payload + Clone,
{
    encode_raw_envelope(RawEnvelope::from_typed(envelope))
        .expect("encode raw envelope")
        .len()
}

fn file_write_request_with_len(bytes_len: usize) -> Envelope<FileWriteRequest> {
    Envelope::new(FileWriteRequest {
        path: "/tmp/m80-frame-max.bin".to_owned(),
        bytes: vec![b'x'; bytes_len],
        mode: Some(0o600),
    })
}

fn exact_max_file_write_request() -> Envelope<FileWriteRequest> {
    let mut low = 0usize;
    let mut high = MAX_FRAME_BYTES;

    while low <= high {
        let mid = low + ((high - low) / 2);
        let envelope = file_write_request_with_len(mid);
        match encoded_body_len(&envelope).cmp(&MAX_FRAME_BYTES) {
            Ordering::Equal => return envelope,
            Ordering::Less => low = mid + 1,
            Ordering::Greater => {
                high = mid
                    .checked_sub(1)
                    .expect("zero-length request exceeded frame cap");
            }
        }
    }

    panic!("could not construct exact MAX_FRAME_BYTES file_write envelope");
}

fn streaming_stdout_request() -> Envelope<ExecRequest> {
    Envelope::with_request_id(
        ExecRequest {
            program: "/bin/sh".to_owned(),
            args: vec![
                "-c".to_owned(),
                "while :; do printf 'm80-before-malformed\\n'; done".to_owned(),
            ],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(30_000),
            streaming: true,
        },
        "malformed-mid-stream".to_owned(),
    )
}

fn write_malformed_frame(raw: &mut BufReader<UnixStream>) {
    raw.get_mut()
        .write_all(&4u32.to_be_bytes())
        .expect("write malformed frame length");
    raw.get_mut()
        .write_all(&[0xff, 0xff, 0xff, 0xff])
        .expect("write malformed frame body");
    raw.get_mut().flush().expect("flush malformed frame");
}

fn assert_console_protocol_error(run_dir: &Path) {
    let console_log = m80_firecracker::console_log_path(run_dir);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let content = std::fs::read_to_string(&console_log).unwrap_or_default();
        if content.contains("protocol_error peer=host-vsock")
            && content.contains("request_id=malformed-mid-stream")
            && content.contains("stream_id=control")
            && content.contains("error_class=malformed_frame")
        {
            return;
        }
        if Instant::now() >= deadline {
            panic!(
                "console log did not expose malformed-frame protocol error at {}:\n{}",
                console_log.display(),
                content
            );
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn frame_length_at_max_bytes_round_trips() {
    let discovery = m80_preflight::run().expect("preflight");
    let (running, run_dir) = launch_vm(&discovery);
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());
    let vm_id = running.vm_id().to_owned();

    let request = exact_max_file_write_request();
    assert_eq!(encoded_body_len(&request), MAX_FRAME_BYTES);
    let bytes_len = request.payload.bytes.len();

    let mut channel = open_channel(&run_dir, &discovery.firecracker_bin, &vm_id);
    channel.send(&request).expect("send exact-max frame");
    let response: Envelope<FileWriteResponse> = channel.recv().expect("recv file_write response");

    assert_eq!(response.payload.error, None);
    assert_eq!(response.payload.bytes_written as usize, bytes_len);

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn frame_length_over_max_drops_current_channel_only() {
    let discovery = m80_preflight::run().expect("preflight");
    let (running, run_dir) = launch_vm(&discovery);
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());
    let vm_id = running.vm_id().to_owned();

    let mut raw = open_raw_stream(&run_dir, &discovery.firecracker_bin, &vm_id);
    raw.get_mut()
        .write_all(&((MAX_FRAME_BYTES + 1) as u32).to_be_bytes())
        .expect("write oversized length prefix");
    raw.get_mut()
        .write_all(b"x")
        .expect("write oversized frame sentinel byte");
    raw.get_mut().flush().expect("flush oversized frame");

    let err = read_frame::<_, PongResponse>(&mut raw)
        .expect_err("oversized input frame must not produce a response");
    assert!(
        matches!(err, ProtoError::Io(_)),
        "oversized input should close/reset the current channel, got {err:?}"
    );

    let mut fresh = open_channel(&run_dir, &discovery.firecracker_bin, &vm_id);
    fresh
        .send(&Envelope::new(PingRequest {}))
        .expect("send fresh ping after oversized drop");
    let pong: Envelope<PongResponse> = fresh.recv().expect("recv fresh pong");
    assert!(pong.payload.guest_unix_ms > 0);

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn malformed_frame_mid_stream_maps_to_user_visible_error() {
    let discovery = m80_preflight::run().expect("preflight");
    let (running, run_dir) = launch_vm(&discovery);
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());
    let vm_id = running.vm_id().to_owned();

    let mut raw = open_raw_stream(&run_dir, &discovery.firecracker_bin, &vm_id);
    write_frame(&mut raw.get_mut(), &streaming_stdout_request()).expect("write streaming exec");

    let first = read_raw_frame(&mut raw).expect("recv first streaming frame");
    assert_eq!(first.kind, PAYLOAD_KIND_EXEC_STDOUT);
    let stdout = first
        .decode::<m80_proto::ExecStdout>()
        .expect("decode stdout");
    assert!(
        stdout.payload.bytes.starts_with(b"m80-before-malformed\n"),
        "unexpected first stdout chunk: {:?}",
        String::from_utf8_lossy(&stdout.payload.bytes)
    );

    write_malformed_frame(&mut raw);

    let mut saw_teardown = None;
    for _ in 0..256 {
        match read_raw_frame(&mut raw) {
            Ok(frame) if frame.kind == PAYLOAD_KIND_EXEC_STDOUT => {}
            Ok(frame) if frame.kind == PAYLOAD_KIND_EXEC_EXIT => {
                let exit = frame.decode::<ExecExit>().expect("decode exec exit");
                panic!("malformed control frame produced terminal exit: {exit:?}");
            }
            Ok(frame) => panic!("unexpected frame after malformed control frame: {frame:?}"),
            Err(err) => {
                saw_teardown = Some(err);
                break;
            }
        }
    }
    let err = saw_teardown.expect("malformed frame did not tear down current channel");
    assert!(
        matches!(err, ProtoError::Io(_)),
        "current channel teardown should surface as I/O error, got {err:?}"
    );

    assert_console_protocol_error(&run_dir);

    let mut fresh = open_channel(&run_dir, &discovery.firecracker_bin, &vm_id);
    fresh
        .send(&Envelope::new(PingRequest {}))
        .expect("send fresh ping after malformed streaming frame");
    let pong: Envelope<PongResponse> = fresh.recv().expect("recv fresh pong");
    assert!(pong.payload.guest_unix_ms > 0);

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn protocol_version_mismatch_returns_failed_response_and_fresh_channel_survives() {
    let discovery = m80_preflight::run().expect("preflight");
    let (running, run_dir) = launch_vm(&discovery);
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());
    let vm_id = running.vm_id().to_owned();

    let mut raw = open_raw_stream(&run_dir, &discovery.firecracker_bin, &vm_id);
    let ping = Envelope::new(PingRequest {});
    let mut body =
        encode_raw_envelope(RawEnvelope::from_typed(&ping)).expect("encode current-version ping");
    assert_eq!(body[0], 0x08, "protobuf field 1 must be version");
    assert_eq!(body[1], PROTOCOL_VERSION as u8);
    body[1] = (PROTOCOL_VERSION + 1) as u8;
    raw.get_mut()
        .write_all(&(body.len() as u32).to_be_bytes())
        .expect("write future-version length");
    raw.get_mut()
        .write_all(&body)
        .expect("write future-version body");
    raw.get_mut().flush().expect("flush future-version frame");

    let response: Envelope<ExecResponse> =
        read_frame(&mut raw).expect("recv protocol mismatch response");
    assert_eq!(response.payload.status, ExecStatus::Failed);
    let stderr = String::from_utf8(response.payload.stderr).expect("stderr utf8");
    assert!(
        stderr.contains("protocol version mismatch"),
        "stderr did not describe version mismatch: {stderr:?}"
    );
    assert!(
        stderr.contains(&format!("expected {PROTOCOL_VERSION}")),
        "stderr did not carry expected version: {stderr:?}"
    );
    assert!(
        stderr.contains(&format!("got {}", PROTOCOL_VERSION + 1)),
        "stderr did not carry observed version: {stderr:?}"
    );

    let mut fresh = open_channel(&run_dir, &discovery.firecracker_bin, &vm_id);
    fresh
        .send(&Envelope::new(PingRequest {}))
        .expect("send fresh ping after future-version rejection");
    let pong: Envelope<PongResponse> = fresh.recv().expect("recv fresh pong");
    assert!(pong.payload.guest_unix_ms > 0);

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}
