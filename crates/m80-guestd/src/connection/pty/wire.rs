//! PTY wire-frame helpers.

use std::io::{BufRead, Write};

use m80_proto::{
    read_raw_frame, write_frame, CancelAck, CancelRequest, CancelStatus, Envelope, ExecStatus,
    PtyControl, PtyExit, PtyInput, PtyOutput, PtyResize, PAYLOAD_KIND_CANCEL_REQUEST,
    PAYLOAD_KIND_PTY_CONTROL, PAYLOAD_KIND_PTY_INPUT, PAYLOAD_KIND_PTY_RESIZE,
};

use super::super::{failed_timing, protocol_log, write_payload_frame};
use crate::guest_log::GuestLogPhase;

pub(super) enum HostFrame {
    None,
    Disconnect,
    Input(PtyInput),
    Resize(PtyResize),
    Control(PtyControl),
    Cancel(CancelRequest),
    Other,
}

pub(super) fn poll_host_frame<R>(
    reader: &mut R,
    reader_ready: &mut impl FnMut(&mut R) -> bool,
    request_id: Option<&str>,
) -> HostFrame
where
    R: BufRead,
{
    if !reader_ready(reader) {
        return HostFrame::None;
    }

    match reader.fill_buf() {
        Ok([]) => HostFrame::Disconnect,
        Ok(_) => {
            let next = match read_raw_frame(reader) {
                Ok(env) => env,
                Err(e) => {
                    protocol_log::warn_proto_error(
                        GuestLogPhase::Exec,
                        request_id,
                        Some("pty_control"),
                        &e,
                    );
                    return HostFrame::Disconnect;
                }
            };
            match next.kind.as_str() {
                PAYLOAD_KIND_PTY_INPUT => match next.decode::<PtyInput>() {
                    Ok(env) => HostFrame::Input(env.payload),
                    Err(e) => {
                        protocol_log::warn_proto_error(
                            GuestLogPhase::Exec,
                            request_id,
                            Some(PAYLOAD_KIND_PTY_INPUT),
                            &e,
                        );
                        HostFrame::Other
                    }
                },
                PAYLOAD_KIND_PTY_RESIZE => match next.decode::<PtyResize>() {
                    Ok(env) => HostFrame::Resize(env.payload),
                    Err(e) => {
                        protocol_log::warn_proto_error(
                            GuestLogPhase::Exec,
                            request_id,
                            Some(PAYLOAD_KIND_PTY_RESIZE),
                            &e,
                        );
                        HostFrame::Other
                    }
                },
                PAYLOAD_KIND_PTY_CONTROL => match next.decode::<PtyControl>() {
                    Ok(env) => HostFrame::Control(env.payload),
                    Err(e) => {
                        protocol_log::warn_proto_error(
                            GuestLogPhase::Exec,
                            request_id,
                            Some(PAYLOAD_KIND_PTY_CONTROL),
                            &e,
                        );
                        HostFrame::Other
                    }
                },
                PAYLOAD_KIND_CANCEL_REQUEST => match next.decode::<CancelRequest>() {
                    Ok(env) => HostFrame::Cancel(env.payload),
                    Err(e) => {
                        protocol_log::warn_proto_error(
                            GuestLogPhase::Exec,
                            request_id,
                            Some(PAYLOAD_KIND_CANCEL_REQUEST),
                            &e,
                        );
                        HostFrame::Other
                    }
                },
                _ => {
                    protocol_log::warn_unexpected_frame(
                        GuestLogPhase::Exec,
                        request_id,
                        Some("pty_control"),
                        next.kind.as_str(),
                    );
                    HostFrame::Other
                }
            }
        }
        Err(_) => HostFrame::Disconnect,
    }
}

pub(super) fn write_cancel_ack<W: Write>(
    writer: &mut W,
    request_id: String,
    status: CancelStatus,
) -> Result<(), m80_proto::ProtoError> {
    let ack = CancelAck { request_id, status };
    let env = Envelope::new(ack);
    write_frame(writer, &env)?;
    writer.flush()?;
    Ok(())
}

pub(super) fn write_pty_failed<W: Write>(
    writer: &mut W,
    request_id: &Option<String>,
    received_at: u64,
    message: String,
) {
    let stderr = message.into_bytes();
    let output_len = stderr.len() as u64;
    let _ = write_payload_frame(
        writer,
        request_id,
        PtyOutput {
            seq: 0,
            bytes: stderr,
        },
    );
    let exit = PtyExit {
        status: ExecStatus::Failed,
        exit_code: None,
        exit_signal: None,
        total_input_bytes: 0,
        total_output_bytes: output_len,
        truncated: false,
        timing: failed_timing(received_at),
    };
    let _ = write_payload_frame(writer, request_id, exit);
    nix::unistd::sync();
}
