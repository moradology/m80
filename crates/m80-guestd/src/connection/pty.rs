//! PTY exec implementation for the connection handler.

mod process;
mod wire;

use std::io::{BufRead, Write};
use std::sync::mpsc::{self, TryRecvError};
use std::thread;
use std::time::Instant;

use m80_proto::{
    CancelStatus, Envelope, ExecTiming, PtyControlEvent, PtyExit, PtyOutput, PtyRequest,
};
use portable_pty::{native_pty_system, ExitStatus as PortableExitStatus};

use process::{
    command_builder, join_output_thread, signal_pty_child, spawn_output_thread, terminal_status,
    terminate_pty_child, timeout_deadline, to_portable_size, PtyFrame,
};
use wire::{poll_host_frame, write_cancel_ack, write_pty_failed, HostFrame};
use super::write_payload_frame;

use crate::guest_log::{self, GuestLogPhase};

use super::{unix_ms_now, ConnectionOutcome, POLL_INTERVAL};

pub(super) fn handle_pty_exec<R, W>(
    raw: Envelope<serde_json::Value>,
    mut reader: R,
    writer: &mut W,
    received_at: u64,
    reader_ready: &mut impl FnMut(&mut R) -> bool,
) -> anyhow::Result<ConnectionOutcome>
where
    R: BufRead,
    W: Write,
{
    let request_id = raw.request_id.clone();
    let req: PtyRequest = match serde_json::from_value(raw.payload) {
        Ok(r) => r,
        Err(e) => {
            guest_log::warn(
                GuestLogPhase::Exec,
                request_id.as_deref(),
                format!("malformed pty_request payload: {e:#}"),
            );
            write_pty_failed(writer, &request_id, received_at, format!("{e:#}"));
            return Ok(ConnectionOutcome::Continue);
        }
    };
    guest_log::info(
        GuestLogPhase::Exec,
        request_id.as_deref(),
        format!("pty request accepted: program={}", req.program),
    );

    let spawn_start = unix_ms_now();
    let pty_system = native_pty_system();
    let pair = match pty_system.openpty(to_portable_size(req.size)) {
        Ok(pair) => pair,
        Err(e) => {
            write_pty_failed(
                writer,
                &request_id,
                received_at,
                format!("openpty failed: {e:#}"),
            );
            return Ok(ConnectionOutcome::Continue);
        }
    };

    let cmd = match command_builder(&req) {
        Ok(cmd) => cmd,
        Err(e) => {
            write_pty_failed(
                writer,
                &request_id,
                received_at,
                format!("command setup failed: {e:#}"),
            );
            return Ok(ConnectionOutcome::Continue);
        }
    };

    let mut child = match pair.slave.spawn_command(cmd) {
        Ok(child) => child,
        Err(e) => {
            write_pty_failed(
                writer,
                &request_id,
                received_at,
                format!("spawn failed: {e:#}"),
            );
            return Ok(ConnectionOutcome::Continue);
        }
    };
    drop(pair.slave);
    let spawned_at = unix_ms_now();

    let output_reader = match pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(e) => {
            let _ = terminate_pty_child(child.as_mut());
            write_pty_failed(
                writer,
                &request_id,
                received_at,
                format!("pty reader clone failed: {e:#}"),
            );
            return Ok(ConnectionOutcome::Continue);
        }
    };
    let mut pty_writer = match pair.master.take_writer() {
        Ok(writer) => Some(writer),
        Err(e) => {
            let _ = terminate_pty_child(child.as_mut());
            write_pty_failed(
                writer,
                &request_id,
                received_at,
                format!("pty writer open failed: {e:#}"),
            );
            return Ok(ConnectionOutcome::Continue);
        }
    };

    let (tx, rx) = mpsc::sync_channel::<PtyFrame>(1);
    let mut output_thread = Some(spawn_output_thread(output_reader, tx));
    let deadline = timeout_deadline(req.timeout_ms);
    let mut exit_status: Option<PortableExitStatus> = None;
    let mut timed_out = false;
    let mut output_done = false;
    let mut total_input_bytes = 0u64;
    let mut total_output_bytes = 0u64;

    loop {
        let mut progressed = false;

        if !output_done {
            match rx.try_recv() {
                Ok(PtyFrame::Output { seq, bytes }) => {
                    progressed = true;
                    if let Err(e) =
                        write_payload_frame(writer, &request_id, PtyOutput { seq, bytes })
                    {
                        guest_log::warn(
                            GuestLogPhase::Exec,
                            request_id.as_deref(),
                            format!("pty output frame write failed: {e}"),
                        );
                        let _ = terminate_pty_child(child.as_mut());
                        drop(pty_writer.take());
                        drop(rx);
                        join_output_thread(&mut output_thread);
                        return Ok(ConnectionOutcome::Continue);
                    }
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    output_done = true;
                    total_output_bytes = join_output_thread(&mut output_thread);
                    progressed = true;
                }
            }
        }

        if exit_status.is_none() && !timed_out {
            match child.try_wait() {
                Ok(Some(status)) => {
                    exit_status = Some(status);
                    drop(pty_writer.take());
                    progressed = true;
                }
                Ok(None) => {}
                Err(e) => {
                    guest_log::warn(
                        GuestLogPhase::Exec,
                        request_id.as_deref(),
                        format!("pty child wait polling failed: {e}"),
                    );
                    let _ = terminate_pty_child(child.as_mut());
                    drop(pty_writer.take());
                    drop(rx);
                    join_output_thread(&mut output_thread);
                    return Ok(ConnectionOutcome::Continue);
                }
            }
        }

        if exit_status.is_none() && !timed_out && Instant::now() >= deadline {
            timed_out = true;
            let (_, status) = terminate_pty_child(child.as_mut());
            exit_status = status;
            drop(pty_writer.take());
            progressed = true;
        }

        match poll_host_frame(&mut reader, reader_ready) {
            HostFrame::None => {}
            HostFrame::Other => {
                progressed = true;
            }
            HostFrame::Disconnect => {
                if exit_status.is_none() {
                    let _ = terminate_pty_child(child.as_mut());
                }
                drop(pty_writer.take());
                drop(rx);
                join_output_thread(&mut output_thread);
                return Ok(ConnectionOutcome::Continue);
            }
            HostFrame::Input(input) => {
                progressed = true;
                if let Some(handle) = pty_writer.as_mut() {
                    total_input_bytes = total_input_bytes.saturating_add(input.bytes.len() as u64);
                    if let Err(e) = handle.write_all(&input.bytes).and_then(|()| handle.flush()) {
                        guest_log::warn(
                            GuestLogPhase::Exec,
                            request_id.as_deref(),
                            format!("pty input write failed: {e}"),
                        );
                        let _ = terminate_pty_child(child.as_mut());
                        drop(pty_writer.take());
                        drop(rx);
                        join_output_thread(&mut output_thread);
                        return Ok(ConnectionOutcome::Continue);
                    }
                }
            }
            HostFrame::Resize(resize) => {
                progressed = true;
                if let Err(e) = pair.master.resize(to_portable_size(resize.size)) {
                    guest_log::warn(
                        GuestLogPhase::Exec,
                        request_id.as_deref(),
                        format!("pty resize failed: {e:#}"),
                    );
                }
            }
            HostFrame::Control(control) => {
                progressed = true;
                match control.event {
                    PtyControlEvent::Eof => {
                        drop(pty_writer.take());
                    }
                    PtyControlEvent::Signal { signal } => {
                        let status = signal_pty_child(child.as_mut(), signal);
                        if status == CancelStatus::Failed {
                            guest_log::warn(
                                GuestLogPhase::Exec,
                                request_id.as_deref(),
                                format!("failed to signal pty child: {signal:?}"),
                            );
                        }
                    }
                }
            }
            HostFrame::Cancel(cancel_req) => {
                let matches = request_id.as_deref() == Some(cancel_req.request_id.as_str());
                if !matches {
                    if write_cancel_ack(writer, cancel_req.request_id, CancelStatus::AlreadyExited)
                        .is_err()
                    {
                        let _ = terminate_pty_child(child.as_mut());
                        drop(pty_writer.take());
                        drop(rx);
                        join_output_thread(&mut output_thread);
                        return Ok(ConnectionOutcome::Continue);
                    }
                    continue;
                }

                if exit_status.is_some() {
                    if write_cancel_ack(writer, cancel_req.request_id, CancelStatus::AlreadyExited)
                        .is_err()
                    {
                        drop(pty_writer.take());
                        drop(rx);
                        join_output_thread(&mut output_thread);
                        return Ok(ConnectionOutcome::Continue);
                    }
                    continue;
                }

                let (status, _) = terminate_pty_child(child.as_mut());
                drop(pty_writer.take());
                drop(rx);
                join_output_thread(&mut output_thread);
                if write_cancel_ack(writer, cancel_req.request_id, status).is_err() {
                    guest_log::warn(
                        GuestLogPhase::Exec,
                        request_id.as_deref(),
                        "failed to write pty cancel ack",
                    );
                }
                nix::unistd::sync();
                return Ok(ConnectionOutcome::Continue);
            }
        }

        if output_done && (exit_status.is_some() || timed_out) {
            let exited_at = unix_ms_now();
            let timing = ExecTiming {
                spawned_at_unix_ms: spawned_at,
                exited_at_unix_ms: exited_at,
                spawn_ms: spawned_at.saturating_sub(spawn_start),
                run_ms: exited_at.saturating_sub(spawned_at),
            };
            let (status, exit_code, exit_signal) = terminal_status(timed_out, exit_status);
            let exit = PtyExit {
                status,
                exit_code,
                exit_signal,
                total_input_bytes,
                total_output_bytes,
                truncated: false,
                timing,
            };
            if let Err(e) = write_payload_frame(writer, &request_id, exit) {
                guest_log::warn(
                    GuestLogPhase::Exec,
                    request_id.as_deref(),
                    format!("failed to write pty exit frame: {e}"),
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
