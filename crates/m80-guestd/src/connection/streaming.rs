//! Streaming exec implementation for the connection handler.

use std::io::{BufRead, Read, Write};
use std::process::{Child, ExitStatus};
use std::sync::mpsc::{self, SyncSender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use m80_proto::{
    read_raw_frame, CancelAck, CancelRequest, CancelStatus, Envelope, ExecExit, ExecRequest,
    ExecStatus, ExecStderr, ExecStdout, ExecTiming, PAYLOAD_KIND_CANCEL_REQUEST,
};

use crate::guest_log::{self, GuestLogPhase};

use super::{
    build_child_command, failed_timing, protocol_log, unix_ms_now, write_payload_frame,
    ConnectionOutcome, MAX_TIMEOUT_MS, POLL_INTERVAL,
};

const PROCESS_GROUP_TERM_GRACE: Duration = Duration::from_millis(100);
const STREAM_CHANNEL_BOUND: usize = 1;

enum StreamFrame {
    Stdout { seq: u32, bytes: Vec<u8> },
    Stderr { seq: u32, bytes: Vec<u8> },
}

#[derive(Debug, Clone, Copy)]
enum StreamKind {
    Stdout,
    Stderr,
}

struct StreamThreads {
    stdout: Option<JoinHandle<u64>>,
    stderr: Option<JoinHandle<u64>>,
}

impl StreamThreads {
    fn join(&mut self) -> (u64, u64) {
        let stdout = self
            .stdout
            .take()
            .map(|h| h.join().expect("stdout stream thread panicked"))
            .unwrap_or(0);
        let stderr = self
            .stderr
            .take()
            .map(|h| h.join().expect("stderr stream thread panicked"))
            .unwrap_or(0);
        (stdout, stderr)
    }
}

enum ControlFrame {
    None,
    Disconnect,
    Cancel(CancelRequest),
    Other,
}

pub(super) fn handle_streaming_exec<R, W>(
    req: ExecRequest,
    request_id: Option<String>,
    mut reader: R,
    writer: &mut W,
    received_at: u64,
    reader_ready: &mut impl FnMut(&mut R) -> bool,
) -> anyhow::Result<ConnectionOutcome>
where
    R: BufRead,
    W: Write,
{
    let spawn_start = unix_ms_now();
    let mut child = match build_child_command(&req).spawn() {
        Ok(child) => child,
        Err(e) => {
            write_spawn_failed(
                writer,
                &request_id,
                received_at,
                format!("spawn failed: {e}"),
            );
            return Ok(ConnectionOutcome::Continue);
        }
    };
    let spawned_at = unix_ms_now();

    if let Some(stdin_bytes) = &req.stdin {
        if let Some(mut stdin_handle) = child.stdin.take() {
            if let Err(e) = stdin_handle.write_all(stdin_bytes) {
                write_spawn_failed(
                    writer,
                    &request_id,
                    received_at,
                    format!("stdin write failed: {e}"),
                );
                kill_child(&mut child);
                return Ok(ConnectionOutcome::Continue);
            }
        }
    }

    let stdout_handle = child.stdout.take().expect("stdout piped");
    let stderr_handle = child.stderr.take().expect("stderr piped");
    let (tx, rx) = mpsc::sync_channel::<StreamFrame>(STREAM_CHANNEL_BOUND);
    let mut threads = StreamThreads {
        stdout: Some(spawn_stream_thread(
            stdout_handle,
            StreamKind::Stdout,
            tx.clone(),
        )),
        stderr: Some(spawn_stream_thread(
            stderr_handle,
            StreamKind::Stderr,
            tx.clone(),
        )),
    };
    drop(tx);

    let deadline = timeout_deadline(req.timeout_ms);
    let mut exit_status: Option<ExitStatus> = None;
    let mut timed_out = false;
    let mut streams_done = false;
    let mut stdout_total = 0;
    let mut stderr_total = 0;

    loop {
        let mut progressed = false;

        if !streams_done {
            match rx.try_recv() {
                Ok(frame) => {
                    progressed = true;
                    if let Err(e) = write_stream_frame(writer, &request_id, frame) {
                        guest_log::warn(
                            GuestLogPhase::Exec,
                            request_id.as_deref(),
                            format!("stream frame write failed: {e}"),
                        );
                        kill_child(&mut child);
                        drop(rx);
                        threads.join();
                        return Ok(ConnectionOutcome::Continue);
                    }
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    streams_done = true;
                    (stdout_total, stderr_total) = threads.join();
                    progressed = true;
                }
            }
        }

        if exit_status.is_none() && !timed_out {
            match child.try_wait() {
                Ok(Some(status)) => {
                    exit_status = Some(status);
                    progressed = true;
                }
                Ok(None) => {}
                Err(e) => {
                    guest_log::warn(
                        GuestLogPhase::Exec,
                        request_id.as_deref(),
                        format!("child wait polling failed: {e}"),
                    );
                    kill_child(&mut child);
                    drop(rx);
                    threads.join();
                    return Ok(ConnectionOutcome::Continue);
                }
            }
        }

        if exit_status.is_none() && !timed_out && Instant::now() >= deadline {
            timed_out = true;
            let (_, status) = terminate_child_group(&mut child);
            exit_status = status;
            progressed = true;
        }

        match poll_control_frame(&mut reader, reader_ready, request_id.as_deref()) {
            ControlFrame::None => {}
            ControlFrame::Other => {
                progressed = true;
            }
            ControlFrame::Disconnect => {
                if exit_status.is_none() {
                    kill_child(&mut child);
                }
                drop(rx);
                threads.join();
                return Ok(ConnectionOutcome::Continue);
            }
            ControlFrame::Cancel(cancel_req) => {
                let matches = request_id.as_deref() == Some(cancel_req.request_id.as_str());
                if !matches {
                    if write_cancel_ack(writer, cancel_req.request_id, CancelStatus::AlreadyExited)
                        .is_err()
                    {
                        kill_child(&mut child);
                        drop(rx);
                        threads.join();
                        return Ok(ConnectionOutcome::Continue);
                    }
                    continue;
                }

                if exit_status.is_some() {
                    if write_cancel_ack(writer, cancel_req.request_id, CancelStatus::AlreadyExited)
                        .is_err()
                    {
                        drop(rx);
                        threads.join();
                        return Ok(ConnectionOutcome::Continue);
                    }
                    continue;
                }

                let status = kill_child(&mut child);
                drop(rx);
                threads.join();
                if write_cancel_ack(writer, cancel_req.request_id, status).is_err() {
                    guest_log::warn(
                        GuestLogPhase::Exec,
                        request_id.as_deref(),
                        "failed to write streaming cancel ack",
                    );
                }
                nix::unistd::sync();
                return Ok(ConnectionOutcome::Continue);
            }
        }

        if streams_done && (exit_status.is_some() || timed_out) {
            let exited_at = unix_ms_now();
            let timing = ExecTiming {
                spawned_at_unix_ms: spawned_at,
                exited_at_unix_ms: exited_at,
                spawn_ms: spawned_at.saturating_sub(spawn_start),
                run_ms: exited_at.saturating_sub(spawned_at),
            };
            let (status, exit_code) = terminal_status(timed_out, exit_status);
            let exit = ExecExit {
                status,
                exit_code,
                total_stdout_bytes: stdout_total,
                total_stderr_bytes: stderr_total,
                truncated: false,
                timing,
            };
            if let Err(e) = write_payload_frame(writer, &request_id, exit) {
                guest_log::warn(
                    GuestLogPhase::Exec,
                    request_id.as_deref(),
                    format!("failed to write streaming exit frame: {e}"),
                );
            }
            nix::unistd::sync();
            return Ok(ConnectionOutcome::Continue);
        }

        if !progressed {
            thread::sleep(POLL_INTERVAL);
        }
    }
}

fn spawn_stream_thread<R>(
    reader: R,
    kind: StreamKind,
    tx: SyncSender<StreamFrame>,
) -> JoinHandle<u64>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || capture_stream(reader, kind, tx))
}

fn capture_stream<R: Read>(mut reader: R, kind: StreamKind, tx: SyncSender<StreamFrame>) -> u64 {
    let mut total = 0u64;
    let mut seq = 0u32;
    let mut tmp = [0u8; 4096];

    loop {
        match reader.read(&mut tmp) {
            Ok(0) => return total,
            Ok(n) => {
                total = total.saturating_add(n as u64);
                let bytes = tmp[..n].to_vec();
                let frame = match kind {
                    StreamKind::Stdout => StreamFrame::Stdout { seq, bytes },
                    StreamKind::Stderr => StreamFrame::Stderr { seq, bytes },
                };
                seq = seq.wrapping_add(1);
                if tx.send(frame).is_err() {
                    return total;
                }
            }
            Err(_) => return total,
        }
    }
}

fn write_stream_frame<W: Write>(
    writer: &mut W,
    request_id: &Option<String>,
    frame: StreamFrame,
) -> Result<(), m80_proto::ProtoError> {
    match frame {
        StreamFrame::Stdout { seq, bytes } => {
            write_payload_frame(writer, request_id, ExecStdout { seq, bytes })
        }
        StreamFrame::Stderr { seq, bytes } => {
            write_payload_frame(writer, request_id, ExecStderr { seq, bytes })
        }
    }
}

fn write_cancel_ack<W: Write>(
    writer: &mut W,
    request_id: String,
    status: CancelStatus,
) -> Result<(), m80_proto::ProtoError> {
    let ack = CancelAck { request_id, status };
    let env = Envelope::new(ack);
    use m80_proto::write_frame;
    write_frame(writer, &env)?;
    writer.flush()?;
    Ok(())
}

fn write_spawn_failed<W: Write>(
    writer: &mut W,
    request_id: &Option<String>,
    received_at: u64,
    message: String,
) {
    let stderr = message.into_bytes();
    let stderr_len = stderr.len() as u64;
    let _ = write_payload_frame(
        writer,
        request_id,
        ExecStderr {
            seq: 0,
            bytes: stderr,
        },
    );
    let exit = ExecExit {
        status: ExecStatus::Failed,
        exit_code: None,
        total_stdout_bytes: 0,
        total_stderr_bytes: stderr_len,
        truncated: false,
        timing: failed_timing(received_at),
    };
    let _ = write_payload_frame(writer, request_id, exit);
    nix::unistd::sync();
}

fn poll_control_frame<R>(
    reader: &mut R,
    reader_ready: &mut impl FnMut(&mut R) -> bool,
    request_id: Option<&str>,
) -> ControlFrame
where
    R: BufRead,
{
    if !reader_ready(reader) {
        return ControlFrame::None;
    }

    match reader.fill_buf() {
        Ok([]) => ControlFrame::Disconnect,
        Ok(_) => {
            let next = match read_raw_frame(reader) {
                Ok(env) => env,
                Err(e) => {
                    protocol_log::warn_proto_error(
                        GuestLogPhase::Exec,
                        request_id,
                        Some("control"),
                        &e,
                    );
                    return ControlFrame::Disconnect;
                }
            };
            if next.kind == PAYLOAD_KIND_CANCEL_REQUEST {
                match next.decode::<CancelRequest>() {
                    Ok(env) => ControlFrame::Cancel(env.payload),
                    Err(e) => {
                        protocol_log::warn_proto_error(
                            GuestLogPhase::Exec,
                            request_id,
                            Some(PAYLOAD_KIND_CANCEL_REQUEST),
                            &e,
                        );
                        ControlFrame::Other
                    }
                }
            } else {
                protocol_log::warn_unexpected_frame(
                    GuestLogPhase::Exec,
                    request_id,
                    Some("control"),
                    next.kind.as_str(),
                );
                ControlFrame::Other
            }
        }
        Err(_) => ControlFrame::Disconnect,
    }
}

fn timeout_deadline(timeout_ms: Option<u64>) -> Instant {
    let effective_ms = timeout_ms.unwrap_or(MAX_TIMEOUT_MS).min(MAX_TIMEOUT_MS);
    Instant::now() + Duration::from_millis(effective_ms)
}

fn terminal_status(timed_out: bool, exit_status: Option<ExitStatus>) -> (ExecStatus, Option<i32>) {
    if timed_out {
        return (ExecStatus::TimedOut, None);
    }
    match exit_status.and_then(|s| s.code()) {
        Some(code) => (ExecStatus::Completed, Some(code)),
        None => (ExecStatus::Failed, None),
    }
}

fn kill_child(child: &mut Child) -> CancelStatus {
    terminate_child_group(child).0
}

fn terminate_child_group(child: &mut Child) -> (CancelStatus, Option<ExitStatus>) {
    let pgid = nix::unistd::Pid::from_raw(child.id() as i32);
    let term = signal_process_group(pgid, nix::sys::signal::Signal::SIGTERM);
    thread::sleep(PROCESS_GROUP_TERM_GRACE);
    let kill = signal_process_group(pgid, nix::sys::signal::Signal::SIGKILL);
    let exit_status = child.wait().ok();
    (cancel_status_from_group_signals(term, kill), exit_status)
}

fn signal_process_group(
    pgid: nix::unistd::Pid,
    signal: nix::sys::signal::Signal,
) -> Result<(), nix::errno::Errno> {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(-pgid.as_raw()), signal)
}

fn cancel_status_from_group_signals(
    term: Result<(), nix::errno::Errno>,
    kill: Result<(), nix::errno::Errno>,
) -> CancelStatus {
    match (term, kill) {
        (Err(nix::errno::Errno::ESRCH), Err(nix::errno::Errno::ESRCH)) => {
            CancelStatus::AlreadyExited
        }
        (Err(e), _) if e != nix::errno::Errno::ESRCH => CancelStatus::Failed,
        (_, Err(e)) if e != nix::errno::Errno::ESRCH => CancelStatus::Failed,
        _ => CancelStatus::Cancelled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_frame_channel_is_bounded_to_one_waiting_frame() {
        let (tx, _rx) = mpsc::sync_channel::<StreamFrame>(STREAM_CHANNEL_BOUND);

        assert!(tx
            .try_send(StreamFrame::Stdout {
                seq: 0,
                bytes: vec![1, 2, 3],
            })
            .is_ok());

        let err = tx
            .try_send(StreamFrame::Stderr {
                seq: 0,
                bytes: vec![4, 5, 6],
            })
            .expect_err("second frame must block behind slow host writer");
        assert!(matches!(err, mpsc::TrySendError::Full(_)));
    }
}
