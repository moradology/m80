//! Transport-agnostic connection handler. The generic over `BufRead+Write`
//! lets unit tests drive it with `Cursor` instead of a real vsock stream.

use std::io::{BufRead, Read, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use m80_proto::{
    read_frame, write_frame, Envelope, ExecRequest, ExecResponse, ExecStatus, ExecTiming,
    ShutdownAction, ShutdownRequest, ShutdownResponse, PAYLOAD_KIND_EXEC_REQUEST,
    PAYLOAD_KIND_SHUTDOWN_REQUEST,
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
        PAYLOAD_KIND_EXEC_REQUEST => handle_exec(raw, &mut writer, received_at),
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

fn handle_exec<W: Write>(
    raw: Envelope<serde_json::Value>,
    writer: &mut W,
    received_at: u64,
) -> anyhow::Result<ConnectionOutcome> {
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

    let spawn_start = unix_ms_now();
    let result = exec_request(&req, spawn_start);
    let response = match result {
        Ok(resp) => resp,
        Err(e) => {
            let timing = failed_timing(received_at);
            error_response(format!("{e:#}").into_bytes(), timing)
        }
    };

    let out_env = match request_id {
        Some(id) => Envelope::with_request_id(response, id),
        None => Envelope::new(response),
    };

    if let Err(e) = write_frame(writer, &out_env) {
        tracing::warn!(error = %e, "failed to write response frame");
    }
    if let Err(e) = writer.flush() {
        tracing::warn!(error = %e, "failed to flush response writer");
    }

    nix::unistd::sync();

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

/// Spawn the child, capture output, apply timeout.
fn exec_request(req: &ExecRequest, spawn_start: u64) -> anyhow::Result<ExecResponse> {
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

    // Apply timeout: poll wait_timeout in a loop.
    let timed_out = wait_with_timeout(&mut child, req.timeout_ms);

    if timed_out {
        let _ = child.kill();
    }

    // Reap the child after kill (or natural exit).
    let exit_status = child.wait().ok();
    let exited_at = unix_ms_now();

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
/// expires. Returns `true` on timeout. Uses `Instant` (monotonic) so a
/// wall-clock step (NTP slew, leap second) can't retroactively expire the
/// budget. The clamp prevents a buggy or malicious host from holding the
/// handler + capture threads forever.
fn wait_with_timeout(child: &mut std::process::Child, timeout_ms: Option<u64>) -> bool {
    let effective_ms = timeout_ms.unwrap_or(MAX_TIMEOUT_MS).min(MAX_TIMEOUT_MS);
    let deadline = Instant::now() + Duration::from_millis(effective_ms);
    let poll_interval = Duration::from_millis(10);

    loop {
        match child.try_wait() {
            Ok(Some(_)) => return false, // child exited naturally
            Ok(None) => {}               // still running
            Err(_) => return false,      // error polling — don't kill
        }

        if Instant::now() >= deadline {
            return true; // timed out
        }

        thread::sleep(poll_interval);
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
