//! Real-KVM wire frame size boundaries over Firecracker's vsock UDS.

mod common;

use std::cmp::Ordering;
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use m80_proto::{
    encode_raw_envelope, read_frame, Envelope, FileWriteRequest, FileWriteResponse, PingRequest,
    PongResponse, ProtoError, RawEnvelope, MAX_FRAME_BYTES,
};
use m80_vsock::{Channel, GUEST_PORT_DEFAULT};

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
    encode_raw_envelope(RawEnvelope::from_typed(envelope.clone()))
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
                    .expect("zero-length request exceeded frame cap")
            }
        }
    }

    panic!("could not construct exact MAX_FRAME_BYTES file_write envelope");
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
