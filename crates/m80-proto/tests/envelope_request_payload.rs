//! Bead m80-g3x.1.2 — request envelope carries opaque exec payload.

use std::io::Cursor;

use m80_proto::{Envelope, ExecRequest, PROTOCOL_VERSION, read_frame, write_frame};

#[test]
fn serializes_program_args_env_cwd_timeout() {
    let req = ExecRequest {
        program: "/usr/bin/python3".into(),
        args: vec!["-c".into(), "print('hi')".into()],
        cwd: Some("/workspace/project".into()),
        env: Some(vec![
            ("HOME".into(), "/root".into()),
            ("PATH".into(), "/usr/bin:/bin".into()),
        ]),
        stdin: Some(b"input data\n".to_vec()),
        timeout_ms: Some(30_000),
    };

    let env = Envelope::with_request_id(req.clone(), "test-req-001".into());

    // Serialize via write_frame.
    let mut buf = Vec::new();
    write_frame(&mut buf, &env).expect("write_frame must succeed");

    // Deserialize via read_frame.
    let mut cursor = Cursor::new(&buf);
    let back: Envelope<ExecRequest> = read_frame(&mut cursor).expect("read_frame must succeed");

    // All fields survive the round-trip.
    assert_eq!(back.version, PROTOCOL_VERSION);
    assert_eq!(back.request_id.as_deref(), Some("test-req-001"));
    assert_eq!(back.payload.program, "/usr/bin/python3");
    assert_eq!(back.payload.args, vec!["-c", "print('hi')"]);
    assert_eq!(back.payload.cwd.as_deref(), Some("/workspace/project"));
    assert_eq!(
        back.payload.env,
        Some(vec![
            ("HOME".into(), "/root".into()),
            ("PATH".into(), "/usr/bin:/bin".into()),
        ])
    );
    assert_eq!(back.payload.stdin.as_deref(), Some(b"input data\n".as_ref()));
    assert_eq!(back.payload.timeout_ms, Some(30_000));
}
