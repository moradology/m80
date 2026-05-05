//! Transport-agnostic connection handler. The generic over `BufRead+Write`
//! lets unit tests drive it with `Cursor` instead of a real vsock stream.
//!
//! # Cancel dispatch
//!
//! When an `exec_request` arrives the handler spawns the child process and
//! then enters a **two-path read loop**:
//!
//! 1. The child-wait thread signals completion via a `std::sync::mpsc` channel
//!    (`child_tx`). The main handler thread polls both the child channel and
//!    the vsock reader in short alternating sleeps.
//! 2. If a `cancel_request` frame arrives before the child exits, the handler
//!    reads the child PID from `Arc<Mutex<Option<u32>>>`, sends `SIGKILL`,
//!    waits for the child-wait thread to confirm reap, and replies with
//!    `CancelAck { Cancelled }`. The child-wait thread's pending
//!    `ExecResponse` is then discarded — it is never written to the wire.
//! 3. If the child exits before a cancel arrives the `ExecResponse` is written
//!    normally and the cancel path is never exercised.
//!
//! The vsock channel is NOT multiplexed: only one exec is in flight at a time
//! (enforced by `RunningSandbox`'s `&mut self` API on the host).

use std::io::{BufRead, Read, Write};
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use m80_proto::{
    read_frame, write_frame, CancelAck, CancelRequest, CancelStatus, Envelope, ExecRequest,
    ExecResponse, ExecStatus, ExecTiming, ShutdownAction, ShutdownRequest, ShutdownResponse,
    PAYLOAD_KIND_CANCEL_REQUEST, PAYLOAD_KIND_EXEC_REQUEST, PAYLOAD_KIND_SHUTDOWN_REQUEST,
};

/// Outcome of handling one connection. The main accept loop checks for
/// [`ConnectionOutcome::Shutdown`] and exits the daemon (or invokes
/// poweroff for non-PID-1 deployments).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionOutcome {
    /// Normal exec request handled. Main loop continues to `accept()`.
    Continue,
    /// Shutdown request handled. Main loop terminates the daemon. The
    /// carried action says how the post-ack termination should happen.
    Shutdown(ShutdownAction),
}

/// Per-stream capture limit: 1 MiB.
const CAPTURE_LIMIT: usize = 1 << 20;

/// Hard ceiling on the per-exec timeout, even if the host sends `None` or
/// a larger value. Without this a guest process that never exits would hold
/// a guestd handler thread + its capture threads indefinitely. One hour is
/// long enough for any realistic in-VM build/test workload.
const MAX_TIMEOUT_MS: u64 = 60 * 60 * 1000;

/// How long to sleep between poll iterations in the cancel-aware wait loop.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}

/// Run one request/response cycle on the provided reader/writer.
///
/// Reads the envelope as JSON Value first, peeks at `kind`, then dispatches:
/// - `exec_request` (the v0.1 path): spawn, capture, respond, sync.
/// - `cancel_request`: look up in-flight exec by `request_id`, SIGKILL, ack.
/// - `shutdown_request`: sync, send ack, return [`ConnectionOutcome::Shutdown`].
/// - any other kind: respond with a Failed envelope and `Continue`.
///
/// On any error (malformed frame, spawn failure, …) an `ExecResponse` with
/// `status: Failed` is attempted. If even that write fails the error is logged
/// and the function returns `Ok(Continue)` so the caller can accept the next
/// connection.
pub fn handle_connection<R, W>(mut reader: R, mut writer: W) -> anyhow::Result<ConnectionOutcome>
where
    R: BufRead,
    W: Write,
{
    let received_at = unix_ms_now();

    let raw: Envelope<serde_json::Value> = match read_frame(&mut reader) {
        Ok(env) => env,
        Err(e) => {
            // Malformed frame: try to send a Failed response, then close.
            let timing = failed_timing(received_at);
            let resp = error_response(format!("{e:#}").into_bytes(), timing);
            let out_env = Envelope::new(resp);
            let _ = write_frame(&mut writer, &out_env);
            return Ok(ConnectionOutcome::Continue);
        }
    };

    match raw.kind.as_str() {
        PAYLOAD_KIND_EXEC_REQUEST => handle_exec(raw, reader, &mut writer, received_at),
        PAYLOAD_KIND_CANCEL_REQUEST => handle_cancel_no_exec(raw, &mut writer),
        PAYLOAD_KIND_SHUTDOWN_REQUEST => handle_shutdown(raw, &mut writer, received_at),
        other => {
            let timing = failed_timing(received_at);
            let resp = error_response(
                format!("unknown envelope kind: {other:?}").into_bytes(),
                timing,
            );
            let out_env = Envelope::new(resp);
            let _ = write_frame(&mut writer, &out_env);
            Ok(ConnectionOutcome::Continue)
        }
    }
}

/// Messages the child-wait thread sends back to the exec handler.
enum ChildResult {
    /// Child exited (naturally or via timeout/kill). Carries the response.
    Done(ExecResponse),
}

fn handle_exec<R, W>(
    raw: Envelope<serde_json::Value>,
    mut reader: R,
    writer: &mut W,
    received_at: u64,
) -> anyhow::Result<ConnectionOutcome>
where
    R: BufRead,
    W: Write,
{
    let request_id = raw.request_id.clone();
    let req: ExecRequest = match serde_json::from_value(raw.payload) {
        Ok(r) => r,
        Err(e) => {
            let timing = failed_timing(received_at);
            let resp = error_response(format!("{e:#}").into_bytes(), timing);
            let out_env = Envelope::new(resp);
            let _ = write_frame(writer, &out_env);
            return Ok(ConnectionOutcome::Continue);
        }
    };

    // `child_pid_slot` is shared between this thread (cancel path) and the
    // child-wait thread. The wait thread populates it once the child is
    // spawned so a racing `cancel_request` can look up the PID.
    let child_pid_slot: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));
    let pid_slot_for_thread = Arc::clone(&child_pid_slot);

    // `cancel_tx` lets this thread interrupt the child-wait thread when a
    // `cancel_request` arrives.
    let (cancel_tx, cancel_rx) = mpsc::channel::<()>();
    let (child_tx, child_rx) = mpsc::channel::<ChildResult>();

    let spawn_start = unix_ms_now();
    let thread_req = req.clone();

    thread::spawn(move || {
        let result = exec_request_with_cancel(
            &thread_req,
            spawn_start,
            pid_slot_for_thread,
            cancel_rx,
        );
        let response = match result {
            Ok(resp) => resp,
            Err(e) => {
                let timing = failed_timing(spawn_start);
                error_response(format!("{e:#}").into_bytes(), timing)
            }
        };
        // Ignore send error: the main thread may have already sent CancelAck
        // and moved on.
        let _ = child_tx.send(ChildResult::Done(response));
    });

    // Poll for either: (a) the child finishes, or (b) a cancel frame arrives.
    loop {
        // Check if child finished.
        match child_rx.try_recv() {
            Ok(ChildResult::Done(response)) => {
                let out_env = match &request_id {
                    Some(id) => Envelope::with_request_id(response, id.clone()),
                    None => Envelope::new(response),
                };
                if let Err(e) = write_frame(writer, &out_env) {
                    tracing::warn!(error = %e, "failed to write response frame");
                }
                if let Err(e) = writer.flush() {
                    tracing::warn!(error = %e, "failed to flush response writer");
                }
                nix::unistd::sync();
                return Ok(ConnectionOutcome::Continue);
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                // Thread panicked or dropped channel — surface as failed.
                let timing = failed_timing(received_at);
                let resp = error_response(b"exec thread disconnected".to_vec(), timing);
                let out_env = match &request_id {
                    Some(id) => Envelope::with_request_id(resp, id.clone()),
                    None => Envelope::new(resp),
                };
                let _ = write_frame(writer, &out_env);
                let _ = writer.flush();
                return Ok(ConnectionOutcome::Continue);
            }
        }

        // Try to read the next frame without blocking. We do a non-blocking
        // peek via `fill_buf`: if the buffer is empty the reader would block,
        // so we sleep and try again.
        //
        // This is safe because `BufRead::fill_buf` may return an empty slice
        // when the underlying reader would block; we detect that and sleep.
        let peeked_len = match reader.fill_buf() {
            Ok(buf) => buf.len(),
            Err(_) => 0,
        };

        if peeked_len > 0 {
            // Data is available — read the next frame.
            let next: Envelope<serde_json::Value> = match read_frame(&mut reader) {
                Ok(env) => env,
                Err(_) => {
                    thread::sleep(POLL_INTERVAL);
                    continue;
                }
            };

            if next.kind == PAYLOAD_KIND_CANCEL_REQUEST {
                if let Ok(cancel_req) =
                    serde_json::from_value::<CancelRequest>(next.payload.clone())
                {
                    let matches = request_id.as_deref() == Some(cancel_req.request_id.as_str());
                    if matches {
                        // Signal the child thread to stop, then SIGKILL.
                        let _ = cancel_tx.send(());

                        // Wait until the exec thread has populated the PID
                        // slot (it does so immediately after spawn). In
                        // normal operation this is a very short spin — the
                        // exec thread runs concurrently and will fill the
                        // slot before any meaningful work is done.
                        let status = wait_for_pid_then_kill(&child_pid_slot);
                        // Wait for the child thread to finish reaping.
                        let _ = child_rx.recv();

                        let ack = CancelAck {
                            request_id: cancel_req.request_id,
                            status,
                        };
                        let ack_env = Envelope::new(ack);
                        if let Err(e) = write_frame(writer, &ack_env) {
                            tracing::warn!(error = %e, "failed to write cancel ack");
                        }
                        if let Err(e) = writer.flush() {
                            tracing::warn!(error = %e, "failed to flush cancel ack");
                        }
                        nix::unistd::sync();
                        return Ok(ConnectionOutcome::Continue);
                    } else {
                        // Wrong request_id — process either already exited or
                        // this is a stale cancel from the host.
                        let ack = CancelAck {
                            request_id: cancel_req.request_id,
                            status: CancelStatus::AlreadyExited,
                        };
                        let ack_env = Envelope::new(ack);
                        let _ = write_frame(writer, &ack_env);
                        let _ = writer.flush();
                        // Continue waiting for the child.
                    }
                }
            }
            // Any other frame mid-exec is ignored; the exec continues.
            continue;
        }

        thread::sleep(POLL_INTERVAL);
    }
}

/// Spin until the exec thread has written the child PID into `pid_slot`, then
/// send SIGKILL. Returns [`CancelStatus`].
///
/// The exec thread populates `pid_slot` immediately after `Command::spawn`
/// succeeds. This spin typically completes in a single iteration; it is
/// bounded by the exec thread's spawn latency (low single-digit ms).
/// If `pid_slot` remains `None` after 5 s we assume the spawn failed and
/// return [`CancelStatus::AlreadyExited`].
fn wait_for_pid_then_kill(pid_slot: &Arc<Mutex<Option<u32>>>) -> CancelStatus {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        {
            let guard = pid_slot.lock().expect("pid_slot poisoned");
            if guard.is_some() {
                drop(guard);
                return sigkill_child(pid_slot);
            }
        }
        if Instant::now() >= deadline {
            // Spawn likely failed; the exec thread will produce a Failed
            // response through child_tx.
            return CancelStatus::AlreadyExited;
        }
        thread::sleep(Duration::from_millis(1));
    }
}

/// Send SIGKILL to the child PID stored in `pid_slot` (if any) and return
/// the appropriate [`CancelStatus`].
fn sigkill_child(pid_slot: &Arc<Mutex<Option<u32>>>) -> CancelStatus {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;

    let pid = {
        let guard = pid_slot.lock().expect("pid_slot poisoned");
        *guard
    };

    match pid {
        None => {
            // Child hasn't been spawned yet or already exited before we got
            // the PID — treat as already exited.
            CancelStatus::AlreadyExited
        }
        Some(raw_pid) => {
            let pid = Pid::from_raw(raw_pid as i32);
            match kill(pid, Signal::SIGKILL) {
                Ok(()) => CancelStatus::Cancelled,
                Err(nix::errno::Errno::ESRCH) => {
                    // No such process — already exited.
                    CancelStatus::AlreadyExited
                }
                Err(_) => CancelStatus::Failed,
            }
        }
    }
}

/// Handle a `cancel_request` that arrived when no exec is in flight.
/// Always replies `AlreadyExited`.
fn handle_cancel_no_exec<W: Write>(
    raw: Envelope<serde_json::Value>,
    writer: &mut W,
) -> anyhow::Result<ConnectionOutcome> {
    let cancel_req: CancelRequest = match serde_json::from_value(raw.payload) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "malformed cancel_request payload");
            return Ok(ConnectionOutcome::Continue);
        }
    };
    let ack = CancelAck {
        request_id: cancel_req.request_id,
        status: CancelStatus::AlreadyExited,
    };
    let ack_env = Envelope::new(ack);
    if let Err(e) = write_frame(writer, &ack_env) {
        tracing::warn!(error = %e, "failed to write cancel ack");
    }
    if let Err(e) = writer.flush() {
        tracing::warn!(error = %e, "failed to flush cancel ack");
    }
    Ok(ConnectionOutcome::Continue)
}

fn handle_shutdown<W: Write>(
    raw: Envelope<serde_json::Value>,
    writer: &mut W,
    _received_at: u64,
) -> anyhow::Result<ConnectionOutcome> {
    let request_id = raw.request_id.clone();
    // Best-effort decode of the request body for logging — we proceed even
    // if it fails to parse since the kind field already told us this is
    // a shutdown.
    if let Ok(req) = serde_json::from_value::<ShutdownRequest>(raw.payload) {
        if let Some(reason) = req.reason {
            tracing::info!(reason = %reason, "shutdown requested");
        }
    }

    // Sync filesystems BEFORE sending ack so host knows the state on disk
    // is settled when it sees the response.
    nix::unistd::sync();

    let action = if std::process::id() == 1 {
        ShutdownAction::Exit
    } else {
        ShutdownAction::Poweroff
    };
    let response = ShutdownResponse { action };
    let out_env = match request_id {
        Some(id) => Envelope::with_request_id(response, id),
        None => Envelope::new(response),
    };
    if let Err(e) = write_frame(writer, &out_env) {
        tracing::warn!(error = %e, "failed to write shutdown ack");
    }
    if let Err(e) = writer.flush() {
        tracing::warn!(error = %e, "failed to flush shutdown ack");
    }

    Ok(ConnectionOutcome::Shutdown(action))
}

/// Spawn the child, capture output, apply timeout, and honor cancel signals.
///
/// `pid_slot` is populated with the child PID immediately after spawn so the
/// cancel path can SIGKILL it. `cancel_rx` receives `()` when the host sends
/// a `cancel_request` — the wait loop exits early so the cancel handler can
/// reap the child.
fn exec_request_with_cancel(
    req: &ExecRequest,
    spawn_start: u64,
    pid_slot: Arc<Mutex<Option<u32>>>,
    cancel_rx: mpsc::Receiver<()>,
) -> anyhow::Result<ExecResponse> {
    let mut cmd = Command::new(&req.program);
    cmd.args(&req.args);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    if let Some(cwd) = &req.cwd {
        cmd.current_dir(cwd);
    }

    if let Some(env_pairs) = &req.env {
        cmd.env_clear();
        for (k, v) in env_pairs {
            cmd.env(k, v);
        }
    }

    if req.stdin.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("spawn failed: {e}"))?;

    let spawned_at = unix_ms_now();

    // Publish PID so the cancel path can SIGKILL.
    {
        let mut guard = pid_slot.lock().expect("pid_slot poisoned");
        *guard = Some(child.id());
    }

    // Write stdin if provided, then close it.
    if let Some(stdin_bytes) = &req.stdin {
        if let Some(mut stdin_handle) = child.stdin.take() {
            stdin_handle.write_all(stdin_bytes)?;
            // stdin_handle drops here, closing the pipe
        }
    }

    let stdout_handle = child.stdout.take().expect("stdout piped");
    let stderr_handle = child.stderr.take().expect("stderr piped");

    // Capture stdout and stderr in parallel threads, capped at CAPTURE_LIMIT.
    let stdout_thread = thread::spawn(move || capture_stream(stdout_handle));
    let stderr_thread = thread::spawn(move || capture_stream(stderr_handle));

    // Apply timeout: poll wait_timeout in a loop, also honoring cancel.
    let (timed_out, cancelled) =
        wait_with_timeout_and_cancel(&mut child, req.timeout_ms, &cancel_rx);

    if timed_out {
        let _ = child.kill();
    }
    // If cancelled, the SIGKILL was already sent by the cancel handler — we
    // just need to reap.

    // Reap the child after kill (or natural exit).
    let exit_status = child.wait().ok();
    let exited_at = unix_ms_now();

    // Clear PID slot so a late-arriving cancel sees no target.
    {
        let mut guard = pid_slot.lock().expect("pid_slot poisoned");
        *guard = None;
    }

    let (stdout, stdout_truncated) = stdout_thread.join().expect("stdout thread panicked");
    let (stderr, stderr_truncated) = stderr_thread.join().expect("stderr thread panicked");

    let spawn_ms = spawned_at.saturating_sub(spawn_start);
    let run_ms = exited_at.saturating_sub(spawned_at);

    let timing = ExecTiming {
        spawned_at_unix_ms: spawned_at,
        exited_at_unix_ms: exited_at,
        spawn_ms,
        run_ms,
    };

    let (status, exit_code) = if timed_out {
        (ExecStatus::TimedOut, None)
    } else if cancelled {
        // Cancelled path: response is produced but discarded by the cancel
        // handler (which has already sent CancelAck). Use Cancelled status
        // so the thread's response is internally consistent.
        (ExecStatus::Cancelled, None)
    } else {
        match exit_status.and_then(|s| s.code()) {
            Some(code) => (ExecStatus::Completed, Some(code)),
            // No exit code means the child was killed by a signal (SIGSEGV,
            // SIGKILL, etc.) or the wait itself failed. Either way, the run
            // didn't complete normally — surface as Failed, not Completed.
            None => (ExecStatus::Failed, None),
        }
    };

    let truncated = if stdout_truncated || stderr_truncated {
        Some(true)
    } else {
        None
    };

    Ok(ExecResponse {
        status,
        exit_code,
        stdout,
        stderr,
        truncated,
        timing,
    })
}

/// Read all bytes from `reader` into a buffer, capping at `CAPTURE_LIMIT`.
/// Returns `(bytes, truncated)`.
fn capture_stream<R: Read>(mut reader: R) -> (Vec<u8>, bool) {
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 4096];
    loop {
        match reader.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                let remaining = CAPTURE_LIMIT.saturating_sub(buf.len());
                if remaining == 0 {
                    return (buf, true);
                }
                let to_copy = n.min(remaining);
                buf.extend_from_slice(&tmp[..to_copy]);
                if to_copy < n {
                    return (buf, true);
                }
            }
            Err(_) => break,
        }
    }
    (buf, false)
}

/// Poll for child exit until `timeout_ms` (clamped to [`MAX_TIMEOUT_MS`])
/// expires OR a cancel signal arrives on `cancel_rx`.
///
/// Returns `(timed_out, cancelled)`. At most one of the two is true.
/// Uses `Instant` (monotonic) so wall-clock steps can't retroactively expire
/// the budget.
fn wait_with_timeout_and_cancel(
    child: &mut std::process::Child,
    timeout_ms: Option<u64>,
    cancel_rx: &mpsc::Receiver<()>,
) -> (bool, bool) {
    let effective_ms = timeout_ms.unwrap_or(MAX_TIMEOUT_MS).min(MAX_TIMEOUT_MS);
    let deadline = Instant::now() + Duration::from_millis(effective_ms);

    loop {
        match child.try_wait() {
            Ok(Some(_)) => return (false, false), // child exited naturally
            Ok(None) => {}                         // still running
            Err(_) => return (false, false),       // error polling — don't kill
        }

        // Check for cancel signal (non-blocking).
        if cancel_rx.try_recv().is_ok() {
            return (false, true);
        }

        if Instant::now() >= deadline {
            return (true, false); // timed out
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn failed_timing(received_at: u64) -> ExecTiming {
    let now = unix_ms_now();
    ExecTiming {
        spawned_at_unix_ms: received_at,
        exited_at_unix_ms: now,
        spawn_ms: 0,
        run_ms: now.saturating_sub(received_at),
    }
}

fn error_response(stderr: Vec<u8>, timing: ExecTiming) -> ExecResponse {
    ExecResponse {
        status: ExecStatus::Failed,
        exit_code: None,
        stdout: vec![],
        stderr,
        truncated: None,
        timing,
    }
}
