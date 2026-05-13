//! Real-KVM negative coverage for guest file-operation error surfaces.

mod common;

use std::cmp::Ordering;
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use m80_firecracker::{Backend, BackendConfig, CgroupMode, FcError, NetworkPolicy, SandboxConfig};
use m80_proto::GUEST_PORT_DEFAULT;
use m80_proto::{
    encode_raw_envelope, read_frame, Envelope, FileError, FileWriteBeginRequest,
    FileWriteBeginResponse, FileWriteChunkRequest, FileWriteChunkResponse, FileWriteCommitRequest,
    FileWriteCommitResponse, PingRequest, PongResponse, ProtoError, RawEnvelope, MAX_FRAME_BYTES,
};
use m80_vsock::Channel;

use common::RunDirDumpGuard;

fn launch_vm() -> (m80_firecracker::RunningSandbox, PathBuf, PathBuf) {
    let discovery = m80_preflight::run().expect("preflight");
    let firecracker_bin = discovery.firecracker_bin.clone();
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
            vm_id: Some(common::unique_vm_id("fileop-err")),
            workspace: None,
            network: NetworkPolicy::NoEgress,
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
    (running, run_dir, firecracker_bin)
}

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

fn oversized_chunk_request(upload_id: &str) -> Envelope<FileWriteChunkRequest> {
    let mut low = 0usize;
    let mut high = MAX_FRAME_BYTES + 1024;

    while low <= high {
        let mid = low + ((high - low) / 2);
        let envelope = Envelope::new(FileWriteChunkRequest {
            upload_id: upload_id.to_owned(),
            seq: 0,
            bytes: vec![b'x'; mid],
        });
        match encoded_body_len(&envelope).cmp(&(MAX_FRAME_BYTES + 1)) {
            Ordering::Equal => return envelope,
            Ordering::Less => low = mid + 1,
            Ordering::Greater => {
                high = mid
                    .checked_sub(1)
                    .expect("zero-length chunk exceeded frame cap");
            }
        }
    }

    panic!("could not construct oversized file_write_chunk envelope");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn read_file_nonexistent_path_returns_not_found() {
    let (mut running, run_dir, _firecracker_bin) = launch_vm();
    let _dump_guard = RunDirDumpGuard::new(run_dir);

    let err = running
        .read_file("/tmp/m80-definitely-missing-file", Some(1024))
        .expect_err("missing guest file must fail");

    assert!(matches!(err, FcError::FileOp(FileError::NotFound)));

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn read_file_kernel_protected_returns_io() {
    let (mut running, run_dir, _firecracker_bin) = launch_vm();
    let _dump_guard = RunDirDumpGuard::new(run_dir);

    let err = running
        .read_file("/proc/1/mem", Some(16))
        .expect_err("kernel-protected guest file must fail");

    assert!(matches!(err, FcError::FileOp(FileError::Io)));

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn write_file_readonly_returns_permission_denied() {
    let (mut running, run_dir, _firecracker_bin) = launch_vm();
    let _dump_guard = RunDirDumpGuard::new(run_dir);

    let err = running
        .write_file("/sys/kernel/notes", b"will-not-write".to_vec(), Some(0o600))
        .expect_err("write to read-only kernel notes must fail");

    assert!(matches!(err, FcError::FileOp(FileError::PermissionDenied)));

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn write_file_missing_parent_returns_not_found() {
    let (mut running, run_dir, _firecracker_bin) = launch_vm();
    let _dump_guard = RunDirDumpGuard::new(run_dir);

    let err = running
        .write_file(
            "/tmp/m80-missing-parent/file.txt",
            b"will-not-write".to_vec(),
            Some(0o600),
        )
        .expect_err("write with missing parent must fail");

    assert!(matches!(err, FcError::FileOp(FileError::NotFound)));

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn chunked_write_sequence_gap_returns_invalid_sequence() {
    let (running, run_dir, firecracker_bin) = launch_vm();
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());
    let vm_id = running.vm_id().to_owned();
    let mut channel = open_channel(&run_dir, &firecracker_bin, &vm_id);

    channel
        .send(&Envelope::new(FileWriteBeginRequest {
            path: "/tmp/m80-sequence-gap.bin".to_owned(),
            mode: Some(0o600),
        }))
        .expect("send begin upload");
    let begin: Envelope<FileWriteBeginResponse> = channel.recv().expect("recv begin upload");
    let upload_id = begin.payload.upload_id.expect("upload id");
    assert_eq!(begin.payload.error, None);

    channel
        .send(&Envelope::new(FileWriteChunkRequest {
            upload_id: upload_id.clone(),
            seq: 1,
            bytes: b"gap".to_vec(),
        }))
        .expect("send sequence-gap chunk");
    let chunk: Envelope<FileWriteChunkResponse> =
        channel.recv().expect("recv sequence-gap response");

    assert_eq!(chunk.payload.upload_id, upload_id);
    assert_eq!(chunk.payload.seq, 1);
    assert_eq!(chunk.payload.bytes_written, 0);
    assert_eq!(chunk.payload.error, Some(FileError::InvalidSequence));

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn chunked_write_unknown_commit_returns_not_found() {
    let (running, run_dir, firecracker_bin) = launch_vm();
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());
    let vm_id = running.vm_id().to_owned();
    let mut channel = open_channel(&run_dir, &firecracker_bin, &vm_id);

    channel
        .send(&Envelope::new(FileWriteCommitRequest {
            upload_id: "missing-upload".to_owned(),
        }))
        .expect("send unknown commit");
    let commit: Envelope<FileWriteCommitResponse> =
        channel.recv().expect("recv unknown commit response");

    assert_eq!(commit.payload.bytes_written, 0);
    assert_eq!(commit.payload.error, Some(FileError::NotFound));

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn chunked_write_oversized_frame_drops_channel() {
    let (running, run_dir, firecracker_bin) = launch_vm();
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());
    let vm_id = running.vm_id().to_owned();

    let request = oversized_chunk_request("oversized-upload");
    let body = encode_raw_envelope(RawEnvelope::from_typed(&request)).expect("encode oversized");
    assert_eq!(body.len(), MAX_FRAME_BYTES + 1);

    let mut raw = open_raw_stream(&run_dir, &firecracker_bin, &vm_id);
    raw.get_mut()
        .write_all(&(body.len() as u32).to_be_bytes())
        .expect("write oversized length");
    match raw.get_mut().write_all(&body) {
        Ok(()) => raw.get_mut().flush().expect("flush oversized frame"),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
        Err(e) => panic!("write oversized body: {e}"),
    }

    let err = read_frame::<_, FileWriteChunkResponse>(&mut raw)
        .expect_err("oversized chunk frame must not produce a response");
    assert!(
        matches!(err, ProtoError::Io(_)),
        "oversized chunk should close/reset the current channel, got {err:?}"
    );

    let mut fresh = open_channel(&run_dir, &firecracker_bin, &vm_id);
    fresh
        .send(&Envelope::new(PingRequest {}))
        .expect("send fresh ping after oversized chunk");
    let pong: Envelope<PongResponse> = fresh.recv().expect("recv fresh pong");
    assert!(pong.payload.guest_unix_ms > 0);

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}
