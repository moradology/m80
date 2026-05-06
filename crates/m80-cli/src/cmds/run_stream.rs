use std::io::Write as _;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

use m80_firecracker::{
    ExecChunk, ExecExit, ExecRequest, ExecResponse, ExecStatus, FcError, RunningSandbox,
};
use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::{Handle, Signals};

pub(super) struct RunOutcome<T> {
    pub(super) payload: T,
    pub(super) signal: Option<i32>,
}

pub(super) fn exec_pipe_streaming(
    running: &mut RunningSandbox,
    req: ExecRequest,
) -> Result<RunOutcome<ExecExit>, FcError> {
    let (cancel_rx, signal_guard) = SignalCancellation::install()?;
    let mut stdout = std::io::stdout().lock();
    let mut stderr = std::io::stderr().lock();
    let exit = running.exec_streaming_with_cancel(req, cancel_rx, |chunk| {
        copy_guest_chunk(chunk, &mut stdout, &mut stderr).map_err(FcError::Io)
    })?;
    let signal = signal_guard.observed_signal();
    stdout.flush().map_err(FcError::Io)?;
    stderr.flush().map_err(FcError::Io)?;
    Ok(RunOutcome {
        payload: exit,
        signal,
    })
}

pub(super) fn exec_buffered(
    running: &mut RunningSandbox,
    req: ExecRequest,
) -> Result<RunOutcome<ExecResponse>, FcError> {
    let (cancel_rx, signal_guard) = SignalCancellation::install()?;
    let response = running.exec_with_cancel(req, cancel_rx)?;
    Ok(RunOutcome {
        payload: response,
        signal: signal_guard.observed_signal(),
    })
}

pub(super) fn process_exit_code(
    status: ExecStatus,
    guest_exit_code: Option<i32>,
    signal: Option<i32>,
) -> i32 {
    if status == ExecStatus::Cancelled {
        signal.map(signal_exit_code).unwrap_or(1)
    } else {
        guest_exit_code.unwrap_or(1)
    }
}

fn signal_exit_code(signal: i32) -> i32 {
    128 + signal
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

struct SignalCancellation {
    first_signal: Arc<AtomicI32>,
    handle: Handle,
    thread: Option<JoinHandle<()>>,
}

impl SignalCancellation {
    fn install() -> Result<(mpsc::Receiver<()>, Self), FcError> {
        let (cancel_tx, cancel_rx) = mpsc::channel();
        let mut signals = Signals::new([SIGINT, SIGTERM, SIGHUP]).map_err(FcError::Io)?;
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
            Self {
                first_signal,
                handle,
                thread: Some(thread),
            },
        ))
    }

    fn observed_signal(&self) -> Option<i32> {
        match self.first_signal.load(Ordering::SeqCst) {
            0 => None,
            signal => Some(signal),
        }
    }
}

impl Drop for SignalCancellation {
    fn drop(&mut self) {
        self.handle.close();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
