//! Exec methods for [`RunningSandbox`].

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use m80_observability::Phase;
use m80_proto::{
    CancelAck, CancelRequest, CancelStatus, Envelope, ExecExit, ExecRequest, ExecResponse,
    ExecStatus, ExecStderr, ExecStdout, ExecTiming, Payload, PtyControl, PtyExit, PtyInput,
    PtyOutput, PtyRequest, PtyResize, RawEnvelope, PAYLOAD_KIND_CANCEL_ACK, PAYLOAD_KIND_EXEC_EXIT,
    PAYLOAD_KIND_EXEC_STDERR, PAYLOAD_KIND_EXEC_STDOUT, PAYLOAD_KIND_PTY_EXIT,
    PAYLOAD_KIND_PTY_OUTPUT,
};
use m80_vsock::{Channel, GUEST_PORT_DEFAULT};

use crate::error::FcError;
use crate::lifecycle::monotonic_ns;
use crate::runroot::unix_ms_now;
use crate::timing::phase_event;
use crate::types::{ExecChunk, PtyHostEvent, PtyOutputChunk, RunningSandbox};

const EXEC_OPEN_SEND_RETRIES: usize = 25;
const EXEC_OPEN_SEND_RETRY_SLEEP: Duration = Duration::from_millis(100);
const EXEC_BUFFER_LIMIT: usize = 1 << 20;
const CANCEL_FORWARDER_POLL: Duration = Duration::from_millis(50);

impl RunningSandbox {
    /// Send one exec request to the in-VM daemon and return a buffered
    /// response.
    ///
    /// The VM stays alive after the call returns; sequential execs on the same
    /// `RunningSandbox` share the same filesystem state. Pipelining (concurrent
    /// exec) is not supported — the borrow checker enforces one in-flight exec
    /// at a time via `&mut self`. Call `stop()` (or drop the sandbox) to tear
    /// down the VM.
    ///
    /// Internally this uses [`RunningSandbox::exec_streaming`] and buffers
    /// stdout/stderr up to the existing 1 MiB per-stream cap.
    pub fn exec(&mut self, req: ExecRequest) -> Result<ExecResponse, FcError> {
        self.exec_inner(req, None)
    }

    /// Send one exec request and return a buffered response, cancelling the
    /// in-flight guest child if `cancel_rx` receives a value.
    ///
    /// Cancellation is best effort until guestd acknowledges it. A successful
    /// cancel returns `ExecStatus::Cancelled` with host-observed timing.
    pub fn exec_with_cancel(
        &mut self,
        req: ExecRequest,
        cancel_rx: mpsc::Receiver<()>,
    ) -> Result<ExecResponse, FcError> {
        self.exec_inner(req, Some(cancel_rx))
    }

    fn exec_inner(
        &mut self,
        req: ExecRequest,
        cancel_rx: Option<mpsc::Receiver<()>>,
    ) -> Result<ExecResponse, FcError> {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut truncated = false;

        let exit = match cancel_rx {
            Some(cancel_rx) => self.exec_streaming_inner(req, Some(cancel_rx), |chunk| {
                match chunk {
                    ExecChunk::Stdout { bytes, .. } => {
                        truncated |= append_capped(&mut stdout, &bytes);
                    }
                    ExecChunk::Stderr { bytes, .. } => {
                        truncated |= append_capped(&mut stderr, &bytes);
                    }
                }
                Ok(())
            })?,
            None => self.exec_streaming(req, |chunk| {
                match chunk {
                    ExecChunk::Stdout { bytes, .. } => {
                        truncated |= append_capped(&mut stdout, &bytes);
                    }
                    ExecChunk::Stderr { bytes, .. } => {
                        truncated |= append_capped(&mut stderr, &bytes);
                    }
                }
                Ok(())
            })?,
        };

        Ok(ExecResponse {
            status: exit.status,
            exit_code: exit.exit_code,
            stdout,
            stderr,
            truncated: if truncated || exit.truncated {
                Some(true)
            } else {
                None
            },
            timing: exit.timing,
        })
    }

    /// Send one exec request and call `on_chunk` as stdout/stderr frames
    /// arrive.
    ///
    /// The request is forced to `streaming = true` before it is sent. The call
    /// blocks until guestd returns the terminal [`ExecExit`] frame.
    pub fn exec_streaming(
        &mut self,
        req: ExecRequest,
        on_chunk: impl FnMut(ExecChunk) -> Result<(), FcError>,
    ) -> Result<ExecExit, FcError> {
        self.exec_streaming_inner(req, None, on_chunk)
    }

    /// Send one streaming exec request, cancelling the in-flight guest child
    /// if `cancel_rx` receives a value.
    ///
    /// The cancel frame is sent on a cloned writer for the same vsock
    /// connection. Guestd serializes requests, so opening a second channel
    /// could not interrupt the running child.
    pub fn exec_streaming_with_cancel(
        &mut self,
        req: ExecRequest,
        cancel_rx: mpsc::Receiver<()>,
        on_chunk: impl FnMut(ExecChunk) -> Result<(), FcError>,
    ) -> Result<ExecExit, FcError> {
        self.exec_streaming_inner(req, Some(cancel_rx), on_chunk)
    }

    /// Send one PTY exec request and call `on_output` as merged terminal
    /// output frames arrive.
    ///
    /// Host terminal input, resize, control, and cancellation events are read
    /// from `event_rx` and forwarded on the same vsock connection. The call
    /// blocks until guestd returns the terminal [`PtyExit`] frame.
    pub fn exec_pty(
        &mut self,
        req: PtyRequest,
        event_rx: mpsc::Receiver<PtyHostEvent>,
        mut on_output: impl FnMut(PtyOutputChunk) -> Result<(), FcError>,
    ) -> Result<PtyExit, FcError> {
        if self.idle_timed_out.load(Ordering::Relaxed) {
            return Err(FcError::IdleTimedOut);
        }
        self.last_activity_ns
            .store(monotonic_ns(), Ordering::Relaxed);

        let vsock_uds = self.jail.jail_path.join("vsock.sock");
        let request_id = request_id_for(&self.vm_id, self.request_id.as_deref(), "pty");
        crate::diagnostics::record_owned(
            &mut self.diagnostics,
            Phase::Request,
            &self.vm_id,
            Some(request_id.as_str()),
            "pty request started",
        );
        let envelope = Envelope::with_request_id(req, request_id.clone());
        let mut channel = send_envelope_with_open_retry(&vsock_uds, &self.vm_id, &envelope)?;
        let started_at_unix_ms = unix_ms_now();
        let _event_forwarder = spawn_pty_event_forwarder(&channel, request_id.clone(), event_rx)?;
        let t = Instant::now();
        let mut output_total = 0u64;
        let mut expected_output_seq = 0u32;

        loop {
            let frame = match channel.recv_raw() {
                Ok(frame) => frame,
                Err(e) => {
                    let err = super::protocol::recv_error(e, "pty exec");
                    crate::diagnostics::record_protocol_error(
                        &mut self.diagnostics,
                        &self.vm_id,
                        &request_id,
                        "pty_exit",
                        &err,
                    );
                    return Err(err);
                }
            };
            let kind = frame.kind.clone();
            match kind.as_str() {
                PAYLOAD_KIND_PTY_OUTPUT => {
                    let chunk = decode_frame::<PtyOutput>(frame)?;
                    if let Err(err) =
                        check_stream_sequence("pty_output", &mut expected_output_seq, chunk.seq)
                    {
                        crate::diagnostics::record_protocol_error(
                            &mut self.diagnostics,
                            &self.vm_id,
                            &request_id,
                            "pty_output",
                            &err,
                        );
                        return Err(err);
                    }
                    output_total = output_total.saturating_add(chunk.bytes.len() as u64);
                    on_output(PtyOutputChunk {
                        seq: chunk.seq,
                        bytes: chunk.bytes,
                    })?;
                }
                PAYLOAD_KIND_PTY_EXIT => {
                    let exit = decode_frame::<PtyExit>(frame)?;
                    phase_event("pty_recv", &self.vm_id, t.elapsed());
                    self.last_activity_ns
                        .store(monotonic_ns(), Ordering::Relaxed);
                    crate::diagnostics::record_owned(
                        &mut self.diagnostics,
                        Phase::Request,
                        &self.vm_id,
                        Some(request_id.as_str()),
                        "pty request completed",
                    );
                    return Ok(exit);
                }
                PAYLOAD_KIND_CANCEL_ACK => {
                    let ack = decode_frame::<CancelAck>(frame)?;
                    match ack.status {
                        CancelStatus::Cancelled => {
                            phase_event("pty_cancelled", &self.vm_id, t.elapsed());
                            self.last_activity_ns
                                .store(monotonic_ns(), Ordering::Relaxed);
                            crate::diagnostics::record_owned(
                                &mut self.diagnostics,
                                Phase::Request,
                                &self.vm_id,
                                Some(request_id.as_str()),
                                "pty request cancelled",
                            );
                            return Ok(cancelled_pty_exit(started_at_unix_ms, output_total));
                        }
                        CancelStatus::AlreadyExited => continue,
                        CancelStatus::Failed => {
                            return Err(FcError::Config(format!(
                                "guest failed to cancel pty request {}",
                                ack.request_id
                            )));
                        }
                    }
                }
                other => {
                    let err = super::protocol::unexpected_frame(
                        "pty exec",
                        "pty_output|pty_exit|cancel_ack",
                        other,
                    );
                    crate::diagnostics::record_protocol_error(
                        &mut self.diagnostics,
                        &self.vm_id,
                        &request_id,
                        "pty_control",
                        &err,
                    );
                    return Err(err);
                }
            }
        }
    }

    fn exec_streaming_inner(
        &mut self,
        mut req: ExecRequest,
        cancel_rx: Option<mpsc::Receiver<()>>,
        mut on_chunk: impl FnMut(ExecChunk) -> Result<(), FcError>,
    ) -> Result<ExecExit, FcError> {
        if self.idle_timed_out.load(Ordering::Relaxed) {
            return Err(FcError::IdleTimedOut);
        }
        self.last_activity_ns
            .store(monotonic_ns(), Ordering::Relaxed);

        req.streaming = true;
        let vsock_uds = self.jail.jail_path.join("vsock.sock");
        let request_id = request_id_for(&self.vm_id, self.request_id.as_deref(), "exec");
        crate::diagnostics::record_owned(
            &mut self.diagnostics,
            Phase::Request,
            &self.vm_id,
            Some(request_id.as_str()),
            "exec request started",
        );
        let envelope = Envelope::with_request_id(req, request_id.clone());
        let mut channel = send_envelope_with_open_retry(&vsock_uds, &self.vm_id, &envelope)?;
        let started_at_unix_ms = unix_ms_now();
        let _cancel_forwarder = match cancel_rx {
            Some(cancel_rx) => {
                let mut sender = channel.try_clone_sender().map_err(FcError::Vsock)?;
                let stop = Arc::new(AtomicBool::new(false));
                let stop_for_thread = Arc::clone(&stop);
                let request_id_clone = request_id.clone();
                let handle = std::thread::spawn(move || {
                    while !stop_for_thread.load(Ordering::Relaxed) {
                        match cancel_rx.recv_timeout(CANCEL_FORWARDER_POLL) {
                            Ok(()) => {
                                let _ = sender.send(&Envelope::new(CancelRequest {
                                    request_id: request_id_clone,
                                }));
                                let _ = sender.close();
                                return;
                            }
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                            Err(mpsc::RecvTimeoutError::Disconnected) => return,
                        }
                    }
                });
                Some(CancelForwarder {
                    stop,
                    handle: Some(handle),
                })
            }
            None => None,
        };
        let t = Instant::now();
        let mut stdout_total = 0u64;
        let mut stderr_total = 0u64;
        let mut expected_stdout_seq = 0u32;
        let mut expected_stderr_seq = 0u32;

        loop {
            let frame = match channel.recv_raw() {
                Ok(frame) => frame,
                Err(e) => {
                    let err = super::protocol::recv_error(e, "streaming exec");
                    crate::diagnostics::record_protocol_error(
                        &mut self.diagnostics,
                        &self.vm_id,
                        &request_id,
                        "exec_exit",
                        &err,
                    );
                    return Err(err);
                }
            };
            let kind = frame.kind.clone();
            match kind.as_str() {
                PAYLOAD_KIND_EXEC_STDOUT => {
                    let chunk = decode_frame::<ExecStdout>(frame)?;
                    if let Err(err) =
                        check_stream_sequence("exec_stdout", &mut expected_stdout_seq, chunk.seq)
                    {
                        crate::diagnostics::record_protocol_error(
                            &mut self.diagnostics,
                            &self.vm_id,
                            &request_id,
                            "exec_stdout",
                            &err,
                        );
                        return Err(err);
                    }
                    stdout_total = stdout_total.saturating_add(chunk.bytes.len() as u64);
                    on_chunk(ExecChunk::Stdout {
                        seq: chunk.seq,
                        bytes: chunk.bytes,
                    })?;
                }
                PAYLOAD_KIND_EXEC_STDERR => {
                    let chunk = decode_frame::<ExecStderr>(frame)?;
                    if let Err(err) =
                        check_stream_sequence("exec_stderr", &mut expected_stderr_seq, chunk.seq)
                    {
                        crate::diagnostics::record_protocol_error(
                            &mut self.diagnostics,
                            &self.vm_id,
                            &request_id,
                            "exec_stderr",
                            &err,
                        );
                        return Err(err);
                    }
                    stderr_total = stderr_total.saturating_add(chunk.bytes.len() as u64);
                    on_chunk(ExecChunk::Stderr {
                        seq: chunk.seq,
                        bytes: chunk.bytes,
                    })?;
                }
                PAYLOAD_KIND_EXEC_EXIT => {
                    let exit = decode_frame::<ExecExit>(frame)?;
                    phase_event("exec_recv", &self.vm_id, t.elapsed());
                    self.last_activity_ns
                        .store(monotonic_ns(), Ordering::Relaxed);
                    crate::diagnostics::record_owned(
                        &mut self.diagnostics,
                        Phase::Request,
                        &self.vm_id,
                        Some(request_id.as_str()),
                        "exec request completed",
                    );
                    return Ok(exit);
                }
                PAYLOAD_KIND_CANCEL_ACK => {
                    let ack = decode_frame::<CancelAck>(frame)?;
                    match ack.status {
                        CancelStatus::Cancelled => {
                            phase_event("exec_cancelled", &self.vm_id, t.elapsed());
                            self.last_activity_ns
                                .store(monotonic_ns(), Ordering::Relaxed);
                            crate::diagnostics::record_owned(
                                &mut self.diagnostics,
                                Phase::Request,
                                &self.vm_id,
                                Some(request_id.as_str()),
                                "exec request cancelled",
                            );
                            return Ok(cancelled_exit(
                                started_at_unix_ms,
                                stdout_total,
                                stderr_total,
                            ));
                        }
                        CancelStatus::AlreadyExited => {
                            continue;
                        }
                        CancelStatus::Failed => {
                            return Err(FcError::Config(format!(
                                "guest failed to cancel exec request {}",
                                ack.request_id
                            )));
                        }
                    }
                }
                other => {
                    let err = super::protocol::unexpected_frame(
                        "streaming exec",
                        "exec_stdout|exec_stderr|exec_exit|cancel_ack",
                        other,
                    );
                    crate::diagnostics::record_protocol_error(
                        &mut self.diagnostics,
                        &self.vm_id,
                        &request_id,
                        "exec_stream",
                        &err,
                    );
                    return Err(err);
                }
            }
        }
    }
}

struct CancelForwarder {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Drop for CancelForwarder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

struct PtyEventForwarder {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Drop for PtyEventForwarder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn decode_frame<T: Payload>(frame: RawEnvelope) -> Result<T, FcError> {
    frame
        .decode::<T>()
        .map(|env| env.payload)
        .map_err(super::protocol::proto_error)
}

fn check_stream_sequence(
    stream: &'static str,
    expected: &mut u32,
    got: u32,
) -> Result<(), FcError> {
    if got != *expected {
        return Err(super::protocol::sequence_mismatch(
            stream,
            u64::from(*expected),
            u64::from(got),
        ));
    }
    *expected = expected
        .checked_add(1)
        .ok_or_else(|| FcError::Config(format!("{stream} sequence overflow")))?;
    Ok(())
}

fn append_capped(dst: &mut Vec<u8>, bytes: &[u8]) -> bool {
    let remaining = EXEC_BUFFER_LIMIT.saturating_sub(dst.len());
    if remaining == 0 {
        return !bytes.is_empty();
    }
    let to_copy = bytes.len().min(remaining);
    dst.extend_from_slice(&bytes[..to_copy]);
    to_copy < bytes.len()
}

fn spawn_pty_event_forwarder(
    channel: &Channel,
    request_id: String,
    event_rx: mpsc::Receiver<PtyHostEvent>,
) -> Result<PtyEventForwarder, FcError> {
    let mut sender = channel.try_clone_sender().map_err(FcError::Vsock)?;
    let stop = Arc::new(AtomicBool::new(false));
    let stop_for_thread = Arc::clone(&stop);
    let handle = std::thread::spawn(move || {
        let mut input_seq = 0u32;
        let mut control_seq = 0u32;
        while !stop_for_thread.load(Ordering::Relaxed) {
            match event_rx.recv_timeout(CANCEL_FORWARDER_POLL) {
                Ok(PtyHostEvent::Input(bytes)) => {
                    let frame = PtyInput {
                        seq: input_seq,
                        bytes,
                    };
                    input_seq = input_seq.wrapping_add(1);
                    let _ = sender.send(&Envelope::with_request_id(frame, request_id.clone()));
                }
                Ok(PtyHostEvent::Resize(size)) => {
                    let frame = PtyResize {
                        seq: control_seq,
                        size,
                    };
                    control_seq = control_seq.wrapping_add(1);
                    let _ = sender.send(&Envelope::with_request_id(frame, request_id.clone()));
                }
                Ok(PtyHostEvent::Control(event)) => {
                    let frame = PtyControl {
                        seq: control_seq,
                        event,
                    };
                    control_seq = control_seq.wrapping_add(1);
                    let _ = sender.send(&Envelope::with_request_id(frame, request_id.clone()));
                }
                Ok(PtyHostEvent::Cancel) => {
                    let _ = sender.send(&Envelope::new(CancelRequest { request_id }));
                    let _ = sender.close();
                    return;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    });
    Ok(PtyEventForwarder {
        stop,
        handle: Some(handle),
    })
}

pub(super) fn request_id_for(vm_id: &str, configured: Option<&str>, kind: &str) -> String {
    configured
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{vm_id}-{kind}-{}", monotonic_ns()))
}

fn cancelled_exit(started_at_unix_ms: u64, stdout_total: u64, stderr_total: u64) -> ExecExit {
    let exited_at_unix_ms = unix_ms_now();
    ExecExit {
        status: ExecStatus::Cancelled,
        exit_code: None,
        total_stdout_bytes: stdout_total,
        total_stderr_bytes: stderr_total,
        truncated: false,
        timing: ExecTiming {
            spawned_at_unix_ms: started_at_unix_ms,
            exited_at_unix_ms,
            spawn_ms: 0,
            run_ms: exited_at_unix_ms.saturating_sub(started_at_unix_ms),
        },
    }
}

fn cancelled_pty_exit(started_at_unix_ms: u64, output_total: u64) -> PtyExit {
    let exited_at_unix_ms = unix_ms_now();
    PtyExit {
        status: ExecStatus::Cancelled,
        exit_code: None,
        exit_signal: None,
        total_input_bytes: 0,
        total_output_bytes: output_total,
        truncated: false,
        timing: ExecTiming {
            spawned_at_unix_ms: started_at_unix_ms,
            exited_at_unix_ms,
            spawn_ms: 0,
            run_ms: exited_at_unix_ms.saturating_sub(started_at_unix_ms),
        },
    }
}

pub(super) fn send_envelope_with_open_retry<T>(
    vsock_uds: &Path,
    vm_id: &str,
    envelope: &Envelope<T>,
) -> Result<Channel, FcError>
where
    T: m80_proto::Payload + Clone,
{
    let mut last_error = None;
    for attempt in 1..=EXEC_OPEN_SEND_RETRIES {
        let mut channel = match Channel::open_uds_only(vsock_uds, GUEST_PORT_DEFAULT) {
            Ok(channel) => channel,
            Err(e) if is_transient_exec_open_send_error(&e) && attempt < EXEC_OPEN_SEND_RETRIES => {
                last_error = Some(FcError::Vsock(e));
                std::thread::sleep(EXEC_OPEN_SEND_RETRY_SLEEP);
                continue;
            }
            Err(e) => return Err(FcError::Vsock(e)),
        };
        let t = Instant::now();
        match channel.send(envelope) {
            Ok(()) => {
                phase_event("exec_send", vm_id, t.elapsed());
                return Ok(channel);
            }
            Err(m80_vsock::VsockError::Io(e))
                if e.kind() == std::io::ErrorKind::BrokenPipe
                    && attempt < EXEC_OPEN_SEND_RETRIES =>
            {
                last_error = Some(FcError::Vsock(m80_vsock::VsockError::Io(e)));
                std::thread::sleep(EXEC_OPEN_SEND_RETRY_SLEEP);
            }
            Err(e) => return Err(FcError::Vsock(e)),
        }
    }
    Err(last_error
        .unwrap_or_else(|| FcError::Config("exec send retry exhausted without an error".into())))
}

fn is_transient_exec_open_send_error(err: &m80_vsock::VsockError) -> bool {
    match err {
        m80_vsock::VsockError::HandshakeFailed => true,
        m80_vsock::VsockError::Io(e) => e.kind() == std::io::ErrorKind::BrokenPipe,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;

    #[test]
    fn append_capped_reports_truncation_after_limit() {
        let mut dst = vec![b'a'; EXEC_BUFFER_LIMIT - 2];
        assert!(append_capped(&mut dst, b"abcd"));
        assert_eq!(dst.len(), EXEC_BUFFER_LIMIT);
        assert_eq!(&dst[EXEC_BUFFER_LIMIT - 2..], b"ab");
    }

    #[test]
    fn append_capped_does_not_mark_empty_extra_as_truncated() {
        let mut dst = vec![b'a'; EXEC_BUFFER_LIMIT];
        assert!(!append_capped(&mut dst, b""));
        assert!(append_capped(&mut dst, b"x"));
    }

    #[test]
    fn exec_open_retry_classifies_handshake_failure_as_transient() {
        assert!(is_transient_exec_open_send_error(
            &m80_vsock::VsockError::HandshakeFailed
        ));
    }

    #[test]
    fn exec_open_retry_classifies_broken_pipe_as_transient() {
        let err = m80_vsock::VsockError::Io(io::Error::new(io::ErrorKind::BrokenPipe, "closed"));
        assert!(is_transient_exec_open_send_error(&err));
    }

    #[test]
    fn exec_open_retry_does_not_retry_receive_style_timeouts() {
        let err = m80_vsock::VsockError::Io(io::Error::new(io::ErrorKind::TimedOut, "timeout"));
        assert!(!is_transient_exec_open_send_error(&err));
    }

    #[test]
    fn request_id_uses_caller_value_when_present() {
        assert_eq!(
            request_id_for("vm-1", Some("req-cli"), "exec"),
            "req-cli".to_owned()
        );
    }

    #[test]
    fn request_id_generates_from_vm_and_kind_without_caller_value() {
        let request_id = request_id_for("vm-1", None, "pty");

        assert!(request_id.starts_with("vm-1-pty-"));
    }

    #[test]
    fn cancelled_exit_reports_host_observed_cancel_status() {
        let exit = cancelled_exit(1_000, 7, 11);

        assert_eq!(exit.status, ExecStatus::Cancelled);
        assert_eq!(exit.exit_code, None);
        assert_eq!(exit.total_stdout_bytes, 7);
        assert_eq!(exit.total_stderr_bytes, 11);
        assert!(exit.timing.exited_at_unix_ms >= 1_000);
    }

    #[test]
    fn stream_sequence_accepts_monotonic_chunks() {
        let mut expected = 0;

        check_stream_sequence("exec_stdout", &mut expected, 0).unwrap();
        check_stream_sequence("exec_stdout", &mut expected, 1).unwrap();

        assert_eq!(expected, 2);
    }

    #[test]
    fn stream_sequence_gap_returns_protocol_error() {
        let mut expected = 0;
        let err = check_stream_sequence("exec_stderr", &mut expected, 2).unwrap_err();

        assert!(matches!(
            err,
            FcError::Protocol(crate::error::WireProtocolError::SequenceMismatch {
                stream: "exec_stderr",
                expected: 0,
                got: 2
            })
        ));
        assert_eq!(expected, 0);
    }

    #[test]
    fn stream_sequence_duplicate_returns_protocol_error() {
        let mut expected = 0;
        check_stream_sequence("pty_output", &mut expected, 0).unwrap();
        let err = check_stream_sequence("pty_output", &mut expected, 0).unwrap_err();

        assert!(matches!(
            err,
            FcError::Protocol(crate::error::WireProtocolError::SequenceMismatch {
                stream: "pty_output",
                expected: 1,
                got: 0
            })
        ));
    }
}
