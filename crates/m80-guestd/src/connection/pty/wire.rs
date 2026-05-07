//! PTY wire-frame helpers.

use std::io::{BufRead, Write};

use m80_proto::{
    read_frame, write_frame, CancelAck, CancelRequest, CancelStatus, Envelope, ExecStatus,
    PtyControl, PtyExit, PtyInput, PtyOutput, PtyResize, PAYLOAD_KIND_CANCEL_REQUEST,
    PAYLOAD_KIND_PTY_CONTROL, PAYLOAD_KIND_PTY_INPUT, PAYLOAD_KIND_PTY_RESIZE,
};

use super::super::{failed_timing, write_payload_frame};

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
            let next: Envelope<serde_json::Value> = match read_frame(reader) {
                Ok(env) => env,
                Err(_) => return HostFrame::Disconnect,
            };
            match next.kind.as_str() {
                PAYLOAD_KIND_PTY_INPUT => match serde_json::from_value::<PtyInput>(next.payload) {
                    Ok(input) => HostFrame::Input(input),
                    Err(_) => HostFrame::Other,
                },
                PAYLOAD_KIND_PTY_RESIZE => {
                    match serde_json::from_value::<PtyResize>(next.payload) {
                        Ok(resize) => HostFrame::Resize(resize),
                        Err(_) => HostFrame::Other,
                    }
                }
                PAYLOAD_KIND_PTY_CONTROL => {
                    match serde_json::from_value::<PtyControl>(next.payload) {
                        Ok(control) => HostFrame::Control(control),
                        Err(_) => HostFrame::Other,
                    }
                }
                PAYLOAD_KIND_CANCEL_REQUEST => {
                    match serde_json::from_value::<CancelRequest>(next.payload) {
                        Ok(cancel) => HostFrame::Cancel(cancel),
                        Err(_) => HostFrame::Other,
                    }
                }
                _ => HostFrame::Other,
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
