use std::io::Cursor;

use m80_proto::{
    read_frame, write_frame, Envelope, ExecStatus, ProtoError, PtyControl, PtyControlEvent,
    PtyExit, PtyInput, PtyOutput, PtyRequest, PtyResize, PtySignal, PtySize, MAX_FRAME_BYTES,
    PAYLOAD_KIND_PTY_CONTROL, PAYLOAD_KIND_PTY_EXIT, PAYLOAD_KIND_PTY_INPUT,
    PAYLOAD_KIND_PTY_OUTPUT, PAYLOAD_KIND_PTY_REQUEST, PAYLOAD_KIND_PTY_RESIZE, PROTOCOL_VERSION,
};

mod common;

fn sample_size() -> PtySize {
    PtySize {
        rows: 24,
        cols: 80,
        pixel_width: None,
        pixel_height: None,
    }
}

#[test]
fn pty_request_round_trips_with_initial_size() {
    let env = Envelope::with_request_id(
        PtyRequest {
            program: "/usr/bin/env".into(),
            args: vec!["bash".into()],
            cwd: Some("/workspace".into()),
            env: Some(vec![("TERM".into(), "xterm-256color".into())]),
            timeout_ms: Some(30_000),
            size: PtySize {
                rows: 40,
                cols: 120,
                pixel_width: Some(960),
                pixel_height: Some(640),
            },
        },
        "req-pty-start".into(),
    );

    let mut buf = Vec::new();
    write_frame(&mut buf, &env).expect("write pty request");
    let decoded: Envelope<PtyRequest> =
        read_frame(&mut Cursor::new(&buf)).expect("read pty request");

    assert_eq!(decoded.kind, PAYLOAD_KIND_PTY_REQUEST);
    assert_eq!(decoded.request_id.as_deref(), Some("req-pty-start"));
    assert_eq!(decoded.payload.program, "/usr/bin/env");
    assert_eq!(decoded.payload.size.rows, 40);
    assert_eq!(decoded.payload.size.cols, 120);
    assert_eq!(decoded.payload.size.pixel_width, Some(960));
    assert_eq!(decoded.payload.size.pixel_height, Some(640));
}

#[test]
fn pty_input_output_resize_control_and_exit_round_trip() {
    let req_id = "req-pty-stream".to_owned();

    let input = Envelope::with_request_id(
        PtyInput {
            seq: 0,
            bytes: b"hello\r".to_vec(),
        },
        req_id.clone(),
    );
    let output = Envelope::with_request_id(
        PtyOutput {
            seq: 0,
            bytes: b"hello\r\n".to_vec(),
        },
        req_id.clone(),
    );
    let resize = Envelope::with_request_id(
        PtyResize {
            seq: 1,
            size: PtySize {
                rows: 50,
                cols: 132,
                pixel_width: None,
                pixel_height: None,
            },
        },
        req_id.clone(),
    );
    let control = Envelope::with_request_id(
        PtyControl {
            seq: 2,
            event: PtyControlEvent::Signal {
                signal: PtySignal::Interrupt,
            },
        },
        req_id.clone(),
    );
    let exit = Envelope::with_request_id(
        PtyExit {
            status: ExecStatus::Completed,
            exit_code: Some(0),
            exit_signal: None,
            total_input_bytes: 6,
            total_output_bytes: 7,
            truncated: false,
            timing: common::sample_timing(),
        },
        req_id,
    );

    let mut buf = Vec::new();
    write_frame(&mut buf, &input).expect("write input");
    write_frame(&mut buf, &output).expect("write output");
    write_frame(&mut buf, &resize).expect("write resize");
    write_frame(&mut buf, &control).expect("write control");
    write_frame(&mut buf, &exit).expect("write exit");

    let mut cursor = Cursor::new(&buf);
    let decoded_input: Envelope<PtyInput> = read_frame(&mut cursor).expect("read input");
    let decoded_output: Envelope<PtyOutput> = read_frame(&mut cursor).expect("read output");
    let decoded_resize: Envelope<PtyResize> = read_frame(&mut cursor).expect("read resize");
    let decoded_control: Envelope<PtyControl> = read_frame(&mut cursor).expect("read control");
    let decoded_exit: Envelope<PtyExit> = read_frame(&mut cursor).expect("read exit");

    assert_eq!(decoded_input.kind, PAYLOAD_KIND_PTY_INPUT);
    assert_eq!(decoded_input.payload.seq, 0);
    assert_eq!(decoded_input.payload.bytes, b"hello\r");
    assert_eq!(decoded_output.kind, PAYLOAD_KIND_PTY_OUTPUT);
    assert_eq!(decoded_output.payload.bytes, b"hello\r\n");
    assert_eq!(decoded_resize.kind, PAYLOAD_KIND_PTY_RESIZE);
    assert_eq!(decoded_resize.payload.size.cols, 132);
    assert_eq!(decoded_control.kind, PAYLOAD_KIND_PTY_CONTROL);
    assert_eq!(
        decoded_control.payload.event,
        PtyControlEvent::Signal {
            signal: PtySignal::Interrupt
        }
    );
    assert_eq!(decoded_exit.kind, PAYLOAD_KIND_PTY_EXIT);
    assert_eq!(decoded_exit.payload.status, ExecStatus::Completed);
    assert_eq!(decoded_exit.payload.total_input_bytes, 6);
    assert_eq!(decoded_exit.payload.total_output_bytes, 7);
}

#[test]
fn interleaved_resize_and_input_frames_preserve_order_and_request_id() {
    let request_id = "req-pty-interleave".to_owned();
    let frames = [
        Envelope::with_request_id(
            PtyInput {
                seq: 0,
                bytes: b"a".to_vec(),
            },
            request_id.clone(),
        ),
        Envelope::with_request_id(
            PtyInput {
                seq: 1,
                bytes: b"b".to_vec(),
            },
            request_id.clone(),
        ),
    ];
    let resize = Envelope::with_request_id(
        PtyResize {
            seq: 0,
            size: sample_size(),
        },
        request_id.clone(),
    );
    let eof = Envelope::with_request_id(
        PtyControl {
            seq: 1,
            event: PtyControlEvent::Eof,
        },
        request_id.clone(),
    );

    let mut buf = Vec::new();
    write_frame(&mut buf, &frames[0]).expect("write first input");
    write_frame(&mut buf, &resize).expect("write resize");
    write_frame(&mut buf, &frames[1]).expect("write second input");
    write_frame(&mut buf, &eof).expect("write eof");

    let mut cursor = Cursor::new(&buf);
    let first: Envelope<PtyInput> = read_frame(&mut cursor).expect("read first input");
    let decoded_resize: Envelope<PtyResize> = read_frame(&mut cursor).expect("read resize");
    let second: Envelope<PtyInput> = read_frame(&mut cursor).expect("read second input");
    let decoded_eof: Envelope<PtyControl> = read_frame(&mut cursor).expect("read eof");

    assert_eq!(first.request_id.as_deref(), Some("req-pty-interleave"));
    assert_eq!(first.payload.seq, 0);
    assert_eq!(
        decoded_resize.request_id.as_deref(),
        Some("req-pty-interleave")
    );
    assert_eq!(decoded_resize.payload.seq, 0);
    assert_eq!(second.request_id.as_deref(), Some("req-pty-interleave"));
    assert_eq!(second.payload.seq, 1);
    assert_eq!(
        decoded_eof.request_id.as_deref(),
        Some("req-pty-interleave")
    );
    assert_eq!(decoded_eof.payload.event, PtyControlEvent::Eof);
}

#[test]
fn oversized_pty_output_frame_is_rejected() {
    let env = Envelope::new(PtyOutput {
        seq: 0,
        bytes: vec![b'x'; MAX_FRAME_BYTES],
    });
    let mut buf = Vec::new();

    let err = write_frame(&mut buf, &env).expect_err("oversized pty output must fail");
    assert!(matches!(err, ProtoError::OversizedPayload { size, limit }
        if size > MAX_FRAME_BYTES && limit == MAX_FRAME_BYTES));
}

#[test]
fn malformed_pty_frame_is_rejected() {
    let raw = format!(
        "{{\"version\":{ver},\"kind\":\"pty_output\",\"request_id\":\"req-pty-bad\",\"payload\":{{\"seq\":0,\"bytes\":\"aGk=\",\"extra\":\"nope\"}}}}\n",
        ver = PROTOCOL_VERSION,
    );
    let mut cursor = Cursor::new(raw.into_bytes());

    let err = read_frame::<_, Envelope<PtyOutput>>(&mut cursor)
        .expect_err("unknown pty output payload field must fail");
    assert!(
        matches!(err, ProtoError::MalformedPayload(_)),
        "expected MalformedPayload, got: {err:?}"
    );
}
