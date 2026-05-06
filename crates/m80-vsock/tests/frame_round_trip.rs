//! Verify that send/recv correctly route through m80_proto framing.
//!
//! We send an `Envelope<ExecRequest>` from the server side after the
//! handshake, and receive it via `Channel::recv`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;

use tempfile::tempdir;

use m80_proto::{
    CancelRequest, Envelope, ExecRequest, ExecResponse, ExecStatus, ExecTiming,
    PAYLOAD_KIND_CANCEL_REQUEST,
};
use m80_vsock::{Channel, GUEST_PORT_DEFAULT};

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

#[test]
fn cloned_sender_writes_control_frame_on_same_connection() {
    let dir = tempdir().unwrap();
    let uds_path = dir.path().join("vsock.sock");

    let listener = UnixListener::bind(&uds_path).unwrap();

    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);

        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(line.starts_with("CONNECT "));
        reader.get_mut().write_all(b"OK 22222\n").unwrap();

        let received: Envelope<serde_json::Value> = m80_proto::read_frame(&mut reader).unwrap();
        assert_eq!(received.kind, PAYLOAD_KIND_CANCEL_REQUEST);
        let cancel: CancelRequest = serde_json::from_value(received.payload).unwrap();
        assert_eq!(cancel.request_id, "req-1");
    });

    let channel = Channel::open_uds_only(&uds_path, GUEST_PORT_DEFAULT).unwrap();
    let mut sender = channel.try_clone_sender().unwrap();
    sender
        .send(&Envelope::new(CancelRequest {
            request_id: "req-1".to_owned(),
        }))
        .unwrap();
    sender.close().unwrap();
    drop(channel);

    server.join().unwrap();
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

/// Client sends a request; server echoes it back as a response envelope;
/// client receives and verifies the response.
#[test]
fn send_recv_envelope_round_trips() {
    let dir = tempdir().unwrap();
    let uds_path = dir.path().join("vsock.sock");

    let listener = UnixListener::bind(&uds_path).unwrap();

    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);

        // Complete the Firecracker handshake.
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(line.starts_with("CONNECT "));
        reader.get_mut().write_all(b"OK 22222\n").unwrap();

        // Receive a frame from the client.
        let received: Envelope<ExecRequest> = m80_proto::read_frame(&mut reader).unwrap();
        assert_eq!(received.payload.program, "/bin/echo");

        // Echo back a response envelope.
        let response = Envelope::new(sample_response());
        m80_proto::write_frame(reader.get_mut(), &response).unwrap();
    });

    let mut channel = Channel::open_uds_only(&uds_path, GUEST_PORT_DEFAULT).unwrap();

    // Send request.
    let request = Envelope::new(sample_request());
    channel.send(&request).unwrap();

    // Receive response.
    let response: Envelope<ExecResponse> = channel.recv().unwrap();
    assert_eq!(response.payload.status, ExecStatus::Completed);
    assert_eq!(response.payload.stdout, b"hello\n");
    assert_eq!(response.payload.exit_code, Some(0));

    channel.close().unwrap();
    server.join().unwrap();
}
