//! Exec methods for [`RunningSandbox`].

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use m80_observability::Phase;
use m80_proto::GUEST_PORT_DEFAULT;
use m80_proto::{
    CancelRequest, CancelResponse, CancelStatus, Envelope, ExecExit, ExecRequest, ExecResponse,
    ExecStatus, ExecStderr, ExecStdout, ExecTiming, Payload, PtyControl, PtyExit, PtyInput,
    PtyOutput, PtyRequest, PtyResize, RawEnvelope, PAYLOAD_KIND_CANCEL_RESPONSE,
    PAYLOAD_KIND_EXEC_EXIT, PAYLOAD_KIND_EXEC_STDERR, PAYLOAD_KIND_EXEC_STDOUT,
    PAYLOAD_KIND_PTY_EXIT, PAYLOAD_KIND_PTY_OUTPUT,
};
use m80_vsock::Channel;

use crate::diagnostics::phase_event;
use crate::error::{FcError, WireProtocolError};
use crate::layout::VSOCK_SOCKET;
use crate::lifecycle::monotonic_ns;
use crate::runroot::unix_ms_now;
use crate::types::{ExecChunk, PtyHostEvent, PtyOutputChunk, RunningSandbox};

const EXEC_OPEN_SEND_RETRIES: usize = 25;
const EXEC_OPEN_SEND_RETRY_SLEEP: Duration = Duration::from_millis(100);
const EXEC_BUFFER_LIMIT: usize = 1 << 20;
const CANCEL_FORWARDER_POLL: Duration = Duration::from_millis(50);
const FORWARDER_JOIN_TIMEOUT: Duration = Duration::from_millis(100);

impl RunningSandbox {
    fn begin_exec_activity(&self) -> Result<ExecActivityGuard, FcError> {
        if self.idle_timed_out.load(Ordering::Relaxed) {
            return Err(FcError::IdleTimedOut);
        }
        self.last_activity_ns
            .store(monotonic_ns(), Ordering::Relaxed);
        self.active_execs.fetch_add(1, Ordering::Relaxed);
        Ok(ExecActivityGuard {
            active_execs: Arc::clone(&self.active_execs),
            last_activity_ns: Arc::clone(&self.last_activity_ns),
        })
    }

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
        self.exec_inner(req, None, true, None)
    }

    /// Send one exec request with a call-wide wall-clock budget.
    pub fn exec_with_max_duration(
        &mut self,
        req: ExecRequest,
        max_duration_ms: u64,
    ) -> Result<ExecResponse, FcError> {
        self.exec_inner(req, None, true, Some(max_duration_ms))
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
        self.exec_inner(req, Some(cancel_rx), true, None)
    }

    pub(crate) fn exec_ready_probe(&mut self, req: ExecRequest) -> Result<ExecResponse, FcError> {
        self.exec_inner(req, None, false, None)
    }

    fn exec_inner(
        &mut self,
        req: ExecRequest,
        cancel_rx: Option<mpsc::Receiver<()>>,
        consume_one_shot: bool,
        max_duration_ms: Option<u64>,
    ) -> Result<ExecResponse, FcError> {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut truncated = false;

        let mut buffer_chunk = |chunk: ExecChunk| -> Result<(), FcError> {
            match chunk {
                ExecChunk::Stdout { bytes, .. } => {
                    truncated |= append_capped(&mut stdout, &bytes);
                }
                ExecChunk::Stderr { bytes, .. } => {
                    truncated |= append_capped(&mut stderr, &bytes);
                }
            }
            Ok(())
        };

        let exit = self.exec_streaming_inner(
            req,
            cancel_rx,
            &mut buffer_chunk,
            consume_one_shot,
            max_duration_ms,
        )?;

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
        self.exec_streaming_inner(req, None, on_chunk, true, None)
    }

    /// Send one streaming exec request with a call-wide wall-clock budget.
    pub fn exec_streaming_with_max_duration(
        &mut self,
        req: ExecRequest,
        max_duration_ms: u64,
        on_chunk: impl FnMut(ExecChunk) -> Result<(), FcError>,
    ) -> Result<ExecExit, FcError> {
        self.exec_streaming_inner(req, None, on_chunk, true, Some(max_duration_ms))
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
        self.exec_streaming_inner(req, Some(cancel_rx), on_chunk, true, None)
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
        self.claim_one_shot_exec()?;
        let _activity = self.begin_exec_activity()?;

        let vsock_uds = self.jail.jail_root().join(VSOCK_SOCKET);
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
                    let chunk =
                        decode_frame_for_request::<PtyOutput>(frame, &request_id, "pty output")?;
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
                    let exit = decode_frame_for_request::<PtyExit>(frame, &request_id, "pty exit")?;
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
                PAYLOAD_KIND_CANCEL_RESPONSE => match decode_cancel_ack(frame, "pty", &request_id)?
                {
                    CancelResponseDisposition::Cancelled => {
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
                    CancelResponseDisposition::AlreadyExited => continue,
                    CancelResponseDisposition::Failed(msg) => {
                        record_cancel_ack_failed(
                            &mut self.diagnostics,
                            &self.vm_id,
                            &request_id,
                            "pty",
                            &msg,
                            t.elapsed(),
                        );
                        return Err(FcError::Protocol(WireProtocolError::PeerRejected {
                            context: "pty cancel",
                            detail: msg,
                        }));
                    }
                },
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
        consume_one_shot: bool,
        max_duration_ms: Option<u64>,
    ) -> Result<ExecExit, FcError> {
        if consume_one_shot {
            self.claim_one_shot_exec()?;
        }
        let _activity = self.begin_exec_activity()?;

        req.streaming = true;
        let vsock_uds = self.jail.jail_root().join(VSOCK_SOCKET);
        let request_id = request_id_for(&self.vm_id, self.request_id.as_deref(), "exec");
        crate::diagnostics::record_owned(
            &mut self.diagnostics,
            Phase::Request,
            &self.vm_id,
            Some(request_id.as_str()),
            "exec request started",
        );
        let mut envelope = Envelope::with_request_id(req, request_id.clone());
        if let Some(max_duration_ms) = max_duration_ms {
            envelope = envelope.with_max_duration_ms(max_duration_ms);
        }
        let host_deadline = max_duration_ms.map(host_exec_deadline);
        let mut channel = send_envelope_with_open_retry(&vsock_uds, &self.vm_id, &envelope)?;
        let started_at_unix_ms = unix_ms_now();
        // NOTE — BufReader + cloned-stream concurrency model
        //
        // `channel` holds two handles to the same underlying UnixStream file
        // descriptor:
        //   - `channel.stream`     — the raw write side (via `send`)
        //   - `channel.buf_reader` — a BufReader wrapping a `try_clone`d FD
        //                            used for reading response frames
        //
        // `try_clone_sender` creates yet another clone of the write-side FD,
        // handed to a background thread for out-of-band cancel frames.
        //
        // This is safe at the syscall level: Unix sockets are full-duplex, so
        // concurrent reads and writes on clones of the same FD do not interfere
        // at the kernel level. The BufReader's internal buffer holds bytes
        // already consumed from the kernel; writes go directly to the kernel
        // send buffer and do not touch the BufReader's state, so no corruption
        // occurs.
        //
        // Frame ordering between the read loop and the cancel-sender thread is
        // NOT synchronized here — that is the protocol's responsibility. Guestd
        // processes the cancel request after the current command finishes
        // reading its input, and responds with a `CancelResponse` frame read
        // by the loop below.
        let _cancel_forwarder = match cancel_rx {
            Some(cancel_rx) => {
                let mut sender = channel.try_clone_sender().map_err(FcError::Vsock)?;
                let stop = Arc::new(AtomicBool::new(false));
                let stop_for_thread = Arc::clone(&stop);
                let request_id_clone = request_id.clone();
                let (done_tx, done_rx) = mpsc::channel();
                let handle = std::thread::spawn(move || {
                    let _done = ForwarderDone(done_tx);
                    while !stop_for_thread.load(Ordering::Relaxed) {
                        match cancel_rx.recv_timeout(CANCEL_FORWARDER_POLL) {
                            Ok(()) => {
                                let _ = sender.send(&Envelope::new(CancelRequest {
                                    request_id: request_id_clone,
                                }));
                                return;
                            }
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                            Err(mpsc::RecvTimeoutError::Disconnected) => return,
                        }
                    }
                });
                Some(CancelForwarder {
                    stop,
                    done_rx,
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
            let frame = match recv_raw_for_exec(&mut channel, host_deadline) {
                Ok(frame) => frame,
                Err(e) => {
                    crate::diagnostics::record_protocol_error(
                        &mut self.diagnostics,
                        &self.vm_id,
                        &request_id,
                        "exec_exit",
                        &e,
                    );
                    return Err(e);
                }
            };
            let kind = frame.kind.clone();
            match kind.as_str() {
                PAYLOAD_KIND_EXEC_STDOUT => {
                    let chunk =
                        decode_frame_for_request::<ExecStdout>(frame, &request_id, "exec stdout")?;
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
                    let chunk =
                        decode_frame_for_request::<ExecStderr>(frame, &request_id, "exec stderr")?;
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
                    let exit =
                        match decode_frame_for_request::<ExecExit>(frame, &request_id, "exec exit")
                        {
                            Ok(exit) => exit,
                            Err(err) => {
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
                PAYLOAD_KIND_CANCEL_RESPONSE => {
                    match decode_cancel_ack(frame, "exec", &request_id)? {
                        CancelResponseDisposition::Cancelled => {
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
                        CancelResponseDisposition::AlreadyExited => continue,
                        CancelResponseDisposition::Failed(msg) => {
                            record_cancel_ack_failed(
                                &mut self.diagnostics,
                                &self.vm_id,
                                &request_id,
                                "exec",
                                &msg,
                                t.elapsed(),
                            );
                            return Err(FcError::Protocol(WireProtocolError::PeerRejected {
                                context: "exec cancel",
                                detail: msg,
                            }));
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

    fn claim_one_shot_exec(&mut self) -> Result<(), FcError> {
        claim_one_shot_exec(self.one_shot, &mut self.one_shot_consumed)
    }
}

struct ExecActivityGuard {
    active_execs: Arc<std::sync::atomic::AtomicUsize>,
    last_activity_ns: Arc<std::sync::atomic::AtomicU64>,
}

impl Drop for ExecActivityGuard {
    fn drop(&mut self) {
        self.last_activity_ns
            .store(monotonic_ns(), Ordering::Relaxed);
        self.active_execs.fetch_sub(1, Ordering::Relaxed);
    }
}

#[derive(Clone, Copy)]
struct HostExecDeadline {
    deadline: Instant,
    timeout: Duration,
}

fn host_exec_deadline(max_duration_ms: u64) -> HostExecDeadline {
    let timeout = Duration::from_millis(max_duration_ms);
    HostExecDeadline {
        deadline: Instant::now() + timeout,
        timeout,
    }
}

fn recv_raw_for_exec(
    channel: &mut Channel,
    host_deadline: Option<HostExecDeadline>,
) -> Result<RawEnvelope, FcError> {
    let Some(host_deadline) = host_deadline else {
        return channel
            .recv_raw()
            .map_err(|err| super::protocol::recv_error(err, "streaming exec"));
    };
    match channel.recv_raw_with_deadline(host_deadline.deadline) {
        Ok(Some(frame)) => Ok(frame),
        Ok(None) => Err(FcError::ExecTimeoutHost {
            timeout: host_deadline.timeout,
        }),
        Err(err) => Err(super::protocol::recv_error(err, "streaming exec")),
    }
}

fn claim_one_shot_exec(one_shot: bool, consumed: &mut bool) -> Result<(), FcError> {
    if !one_shot {
        return Ok(());
    }
    if *consumed {
        return Err(FcError::OneShotConsumed);
    }
    *consumed = true;
    Ok(())
}

struct CancelForwarder {
    stop: Arc<AtomicBool>,
    done_rx: mpsc::Receiver<()>,
    handle: Option<JoinHandle<()>>,
}

impl Drop for CancelForwarder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        join_forwarder_with_timeout(
            "cancel forwarder",
            &self.done_rx,
            &mut self.handle,
            FORWARDER_JOIN_TIMEOUT,
        );
    }
}

struct PtyEventForwarder {
    stop: Arc<AtomicBool>,
    done_rx: mpsc::Receiver<()>,
    handle: Option<JoinHandle<()>>,
}

impl Drop for PtyEventForwarder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        join_forwarder_with_timeout(
            "pty event forwarder",
            &self.done_rx,
            &mut self.handle,
            FORWARDER_JOIN_TIMEOUT,
        );
    }
}

struct ForwarderDone(mpsc::Sender<()>);

impl Drop for ForwarderDone {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

fn join_forwarder_with_timeout(
    name: &'static str,
    done_rx: &mpsc::Receiver<()>,
    handle: &mut Option<JoinHandle<()>>,
    timeout: Duration,
) {
    let Some(thread) = handle.take() else {
        return;
    };
    match done_rx.recv_timeout(timeout) {
        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => match thread.join() {
            Ok(()) => {}
            Err(panic) => tracing::error!(?panic, "{name} thread panicked"),
        },
        Err(mpsc::RecvTimeoutError::Timeout) => {
            tracing::error!("{name} thread did not stop within {timeout:?}; detaching");
        }
    }
}

/// Result of decoding a `CANCEL_ACK` frame.
#[derive(Debug)]
enum CancelResponseDisposition {
    /// Guest confirmed the child was cancelled; caller should return a cancel exit.
    Cancelled,
    /// Guest says the child already exited; caller should keep reading frames.
    AlreadyExited,
    /// Guest failed to cancel; caller should return an error.
    Failed(String),
}

/// Decode a raw frame as `CancelResponse` and classify its status.
fn decode_cancel_ack(
    frame: RawEnvelope,
    request_kind: &'static str,
    request_id: &str,
) -> Result<CancelResponseDisposition, FcError> {
    let ack = decode_frame::<CancelResponse>(frame)?;
    if ack.request_id != request_id {
        return Err(super::protocol::request_id_mismatch(
            request_kind,
            request_id,
            Some(ack.request_id),
        ));
    }
    Ok(match ack.status {
        CancelStatus::Cancelled => CancelResponseDisposition::Cancelled,
        CancelStatus::AlreadyExited => CancelResponseDisposition::AlreadyExited,
        CancelStatus::Failed => CancelResponseDisposition::Failed(format!(
            "guest failed to cancel {request_kind} request {}",
            ack.request_id
        )),
    })
}

fn record_cancel_ack_failed(
    diagnostics: &mut Option<m80_observability::Diagnostics>,
    vm_id: &str,
    request_id: &str,
    request_kind: &'static str,
    message: &str,
    elapsed: Duration,
) {
    phase_event(&format!("{request_kind}_cancel_failed"), vm_id, elapsed);
    crate::diagnostics::record_owned(
        diagnostics,
        Phase::Request,
        vm_id,
        Some(request_id),
        message,
    );
}

fn decode_frame<T: Payload>(frame: RawEnvelope) -> Result<T, FcError> {
    frame
        .decode::<T>()
        .map(|env| env.payload)
        .map_err(super::protocol::proto_error)
}

fn decode_frame_for_request<T: Payload>(
    frame: RawEnvelope,
    request_id: &str,
    context: &'static str,
) -> Result<T, FcError> {
    if frame.request_id.as_deref() != Some(request_id) {
        return Err(super::protocol::request_id_mismatch(
            context,
            request_id,
            frame.request_id,
        ));
    }
    decode_frame::<T>(frame)
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
    *expected =
        expected
            .checked_add(1)
            .ok_or(FcError::Protocol(WireProtocolError::SequenceOverflow {
                stream,
            }))?;
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
    let (done_tx, done_rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let _done = ForwarderDone(done_tx);
        let mut input_seq = 0u32;
        let mut control_seq = 0u32;
        // Send errors on the vsock channel are intentionally ignored: a
        // channel-closed result means the guest has already disconnected
        // (normal PTY teardown), and there is no recovery path from within
        // this fire-and-forget forwarder thread.
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
                    return;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    });
    Ok(PtyEventForwarder {
        stop,
        done_rx,
        handle: Some(handle),
    })
}

pub(super) fn request_id_for(vm_id: &str, configured: Option<&str>, kind: &str) -> String {
    // Each exec gets a unique id even when the caller supplied a base
    // `request_id`: cancel routing on the guest indexes by this string, so
    // back-to-back execs on the same RunningSandbox would otherwise collide.
    match configured {
        Some(base) => format!("{base}-{kind}-{}", monotonic_ns()),
        None => format!("{vm_id}-{kind}-{}", monotonic_ns()),
    }
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
            Err(e) if is_transient_exec_open_send_error(&e) && attempt < EXEC_OPEN_SEND_RETRIES => {
                last_error = Some(FcError::Vsock(e));
                std::thread::sleep(EXEC_OPEN_SEND_RETRY_SLEEP);
            }
            Err(e) => return Err(FcError::Vsock(e)),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        FcError::Protocol(WireProtocolError::PeerRejected {
            context: "exec send retry",
            detail: "retry exhausted without an error".to_owned(),
        })
    }))
}

fn is_transient_exec_open_send_error(err: &m80_vsock::VsockError) -> bool {
    match err {
        m80_vsock::VsockError::HandshakeFailed => true,
        m80_vsock::VsockError::Io { source, .. } => matches!(
            source.kind(),
            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionRefused
        ),
        _ => false,
    }
}

#[cfg(test)]
#[path = "exec_tests.rs"]
mod tests;
