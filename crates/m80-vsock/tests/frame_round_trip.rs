//! Verify that send/recv correctly route through m80_proto framing.
//!
//! We send an `Envelope<ExecRequest>` from the server side after the
//! handshake, and receive it via `Channel::recv`.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::time::{Duration, Instant};

use tempfile::TempDir;

use m80_proto::GUEST_PORT_DEFAULT;
use m80_proto::{
    CancelRequest, Envelope, ExecRequest, ExecResponse, ExecStatus, ExecTiming,
    PAYLOAD_KIND_CANCEL_REQUEST,
};
use m80_vsock::Channel;

fn sample_request() -> ExecRequest {
    ExecRequest {
        program: "/bin/echo".into(),
        args: vec!["hello".into()],
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(1_000),
        streaming: false,
    }
}

fn sample_response() -> ExecResponse {
    ExecResponse {
        status: ExecStatus::Completed,
        exit_code: Some(0),
        stdout: b"hello\n".to_vec(),
        stderr: Vec::new(),
        truncated: None,
        timing: ExecTiming {
            spawned_at_unix_ms: 1_000_000,
            exited_at_unix_ms: 1_000_050,
            spawn_ms: 5,
            run_ms: 45,
        },
    }
}

/// Complete the Firecracker UDS handshake on the server side and return
/// the buffered reader for further frame exchange.
fn accept_and_handshake(
    listener: &UnixListener,
    ok_port: u32,
) -> BufReader<std::os::unix::net::UnixStream> {
    let (stream, _) = listener.accept().unwrap();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert!(line.starts_with("CONNECT "));
    let reply = format!("OK {ok_port}\n");
    reader.get_mut().write_all(reply.as_bytes()).unwrap();
    reader
}

#[test]
fn cloned_sender_writes_control_frame_on_same_connection() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("vsock.sock");
    let listener = UnixListener::bind(&path).unwrap();

    let server = std::thread::spawn(move || {
        let mut reader = accept_and_handshake(&listener, 22222);

        let received = m80_proto::read_raw_frame(&mut reader).unwrap();
        assert_eq!(received.kind, PAYLOAD_KIND_CANCEL_REQUEST);
        let cancel = received.decode::<CancelRequest>().unwrap().payload;
        assert_eq!(cancel.request_id, "req-1");
    });

    let channel = Channel::open_uds_only(&path, GUEST_PORT_DEFAULT).unwrap();
    let mut sender = channel.try_clone_sender().unwrap();
    sender
        .send(&Envelope::new(CancelRequest {
            request_id: "req-1".to_owned(),
        }))
        .unwrap();
    drop(sender);
    drop(channel);

    server.join().unwrap();
}

/// Client sends a request; server echoes it back as a response envelope;
/// client receives and verifies the response.
#[test]
fn send_recv_envelope_round_trips() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("vsock.sock");
    let listener = UnixListener::bind(&path).unwrap();

    let server = std::thread::spawn(move || {
        let mut reader = accept_and_handshake(&listener, 22222);

        // Receive a frame from the client.
        let received: Envelope<ExecRequest> = m80_proto::read_frame(&mut reader).unwrap();
        assert_eq!(received.payload.program, "/bin/echo");

        // Echo back a response envelope.
        let response = Envelope::new(sample_response());
        m80_proto::write_frame(reader.get_mut(), &response).unwrap();
    });

    let mut channel = Channel::open_uds_only(&path, GUEST_PORT_DEFAULT).unwrap();

    // Send request.
    let request = Envelope::new(sample_request());
    channel.send(&request).unwrap();

    // Receive response.
    let response: Envelope<ExecResponse> = channel.recv().unwrap();
    assert_eq!(response.payload.status, ExecStatus::Completed);
    assert_eq!(response.payload.stdout, b"hello\n");
    assert_eq!(response.payload.exit_code, Some(0));

    drop(channel);
    server.join().unwrap();
}

#[test]
fn recv_raw_with_deadline_expires_during_slow_drip_frame() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("vsock.sock");
    let listener = UnixListener::bind(&path).unwrap();

    let server = std::thread::spawn(move || {
        let mut reader = accept_and_handshake(&listener, 22222);
        for byte in [0, 0, 0, 8, b's', b'l', b'o', b'w'] {
            let _ = reader.get_mut().write_all(&[byte]);
            let _ = reader.get_mut().flush();
            std::thread::sleep(Duration::from_millis(30));
        }
    });

    let mut channel = Channel::open_uds_only(&path, GUEST_PORT_DEFAULT).unwrap();
    let started = Instant::now();
    let frame = channel
        .recv_raw_with_deadline(Instant::now() + Duration::from_millis(50))
        .unwrap();

    assert!(frame.is_none());
    assert!(started.elapsed() < Duration::from_secs(1));

    drop(channel);
    server.join().unwrap();
}
