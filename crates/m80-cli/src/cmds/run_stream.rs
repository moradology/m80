use std::io::Write as _;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{mpsc, Arc};

use m80_firecracker::{ExecChunk, FcError, RunningSandbox};
use m80_proto::{ExecExit, ExecRequest, ExecResponse, ExecStatus};
use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::Signals;

use super::signal_watcher::SignalWatcher;
use crate::errors;

pub(super) struct RunOutcome<T> {
    pub(super) payload: T,
    pub(super) signal: Option<i32>,
}

pub(super) fn exec_pipe_streaming(
    running: &mut RunningSandbox,
    req: ExecRequest,
) -> Result<RunOutcome<ExecExit>, FcError> {
    let (cancel_rx, watcher) = SignalCancellation::install()?;
    let mut stdout = std::io::stdout().lock();
    let mut stderr = std::io::stderr().lock();
    let exit = running.exec_streaming_with_cancel(req, cancel_rx, |chunk| {
        copy_guest_chunk(chunk, &mut stdout, &mut stderr)
            .map_err(|source| errors::host_io("write streamed guest output", source))
    })?;
    let signal = watcher.observed_signal();
    stdout
        .flush()
        .map_err(|source| errors::host_io("flush stdout", source))?;
    stderr
        .flush()
        .map_err(|source| errors::host_io("flush stderr", source))?;
    Ok(RunOutcome {
        payload: exit,
        signal,
    })
}

pub(super) fn exec_buffered(
    running: &mut RunningSandbox,
    req: ExecRequest,
) -> Result<RunOutcome<ExecResponse>, FcError> {
    let (cancel_rx, watcher) = SignalCancellation::install()?;
    let response = running.exec_with_cancel(req, cancel_rx)?;
    Ok(RunOutcome {
        payload: response,
        signal: watcher.observed_signal(),
    })
}

pub(super) fn process_exit_code(
    status: ExecStatus,
    guest_exit_code: Option<i32>,
    signal: Option<i32>,
) -> i32 {
    if status == ExecStatus::Cancelled {
        signal.map(|s| 128 + s).unwrap_or(1)
    } else {
        guest_exit_code.unwrap_or(1)
    }
}

pub(super) fn copy_guest_chunk<W, E>(
    chunk: ExecChunk,
    stdout: &mut W,
    stderr: &mut E,
) -> std::io::Result<()>
where
    W: std::io::Write,
    E: std::io::Write,
{
    match chunk {
        ExecChunk::Stdout { bytes, .. } => {
            stdout.write_all(&bytes)?;
            stdout.flush()
        }
        ExecChunk::Stderr { bytes, .. } => {
            stderr.write_all(&bytes)?;
            stderr.flush()
        }
    }
}

struct SignalCancellation(SignalWatcher);

impl SignalCancellation {
    fn install() -> Result<(mpsc::Receiver<()>, Self), FcError> {
        let (cancel_tx, cancel_rx) = mpsc::channel();
        let mut signals = Signals::new([SIGINT, SIGTERM, SIGHUP])
            .map_err(|source| errors::host_io("install run signal handler", source))?;
        let handle = signals.handle();
        let first_signal = Arc::new(AtomicI32::new(0));
        let first_signal_for_thread = Arc::clone(&first_signal);
        let thread = std::thread::spawn(move || {
            for signal in signals.forever() {
                if first_signal_for_thread
                    .compare_exchange(0, signal, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    let _ = cancel_tx.send(());
                }
            }
        });
        Ok((
            cancel_rx,
            Self(SignalWatcher::new(first_signal, handle, thread)),
        ))
    }

    fn observed_signal(&self) -> Option<i32> {
        self.0.observed_signal()
    }
}
