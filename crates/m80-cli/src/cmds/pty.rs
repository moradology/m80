use std::io::{Read as _, Write as _};
use std::os::fd::AsFd;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

use m80_firecracker::{FcError, PtyHostEvent, PtyOutputChunk, RunningSandbox};
use m80_proto::{PtyControlEvent, PtyExit, PtyRequest, PtySize};
use nix::sys::termios::{self, SetArg, Termios};
use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM, SIGWINCH};
use signal_hook::iterator::{Handle, Signals};
use terminal_size::{terminal_size, Height, Width};

use super::run_stream::{process_exit_code, RunOutcome};

pub(super) fn exec_pty_streaming(
    running: &mut RunningSandbox,
    req: PtyRequest,
    interactive: bool,
) -> Result<RunOutcome<PtyExit>, FcError> {
    let (event_tx, event_rx) = mpsc::channel();
    let signal_guard = PtySignalForwarder::install(event_tx.clone())?;
    let mut terminal = HostTerminalMode::default();
    let _raw_guard = if interactive {
        Some(RawModeGuard::enter(&mut terminal).map_err(FcError::Io)?)
    } else {
        None
    };
    let _input_thread = if interactive {
        Some(InputThread::spawn(event_tx))
    } else {
        None
    };
    let mut stdout = std::io::stdout().lock();
    let exit = running.exec_pty(req, event_rx, |chunk| {
        copy_terminal_output(chunk, &mut stdout).map_err(FcError::Io)
    })?;
    let signal = signal_guard.observed_signal();
    stdout.flush().map_err(FcError::Io)?;
    Ok(RunOutcome {
        payload: exit,
        signal,
    })
}

pub(super) fn pty_exit_code(exit: &PtyExit, signal: Option<i32>) -> i32 {
    process_exit_code(exit.status, exit.exit_code, signal)
}

pub(super) fn current_pty_size() -> PtySize {
    match terminal_size() {
        Some((Width(cols), Height(rows))) => PtySize {
            rows,
            cols,
            pixel_width: None,
            pixel_height: None,
        },
        None => PtySize {
            rows: 24,
            cols: 80,
            pixel_width: None,
            pixel_height: None,
        },
    }
}

pub(super) fn copy_terminal_output<W: std::io::Write>(
    chunk: PtyOutputChunk,
    stdout: &mut W,
) -> std::io::Result<()> {
    stdout.write_all(&chunk.bytes)?;
    stdout.flush()
}

pub(super) trait TerminalMode {
    fn enable_raw(&mut self) -> std::io::Result<()>;
    fn restore(&mut self) -> std::io::Result<()>;
}

pub(super) struct RawModeGuard<'a, T: TerminalMode + ?Sized> {
    terminal: &'a mut T,
    active: bool,
}

impl<'a, T: TerminalMode + ?Sized> RawModeGuard<'a, T> {
    pub(super) fn enter(terminal: &'a mut T) -> std::io::Result<Self> {
        terminal.enable_raw()?;
        Ok(Self {
            terminal,
            active: true,
        })
    }
}

impl<T: TerminalMode + ?Sized> Drop for RawModeGuard<'_, T> {
    fn drop(&mut self) {
        if self.active {
            let _ = self.terminal.restore();
            self.active = false;
        }
    }
}

#[derive(Default)]
struct HostTerminalMode {
    original: Option<Termios>,
}

impl TerminalMode for HostTerminalMode {
    fn enable_raw(&mut self) -> std::io::Result<()> {
        let stdin = std::io::stdin();
        let original = termios::tcgetattr(stdin.as_fd()).map_err(errno_to_io)?;
        let mut raw = original.clone();
        termios::cfmakeraw(&mut raw);
        termios::tcsetattr(stdin.as_fd(), SetArg::TCSANOW, &raw).map_err(errno_to_io)?;
        self.original = Some(original);
        Ok(())
    }

    fn restore(&mut self) -> std::io::Result<()> {
        if let Some(original) = self.original.take() {
            let stdin = std::io::stdin();
            termios::tcsetattr(stdin.as_fd(), SetArg::TCSANOW, &original).map_err(errno_to_io)?;
        }
        Ok(())
    }
}

fn errno_to_io(errno: nix::errno::Errno) -> std::io::Error {
    std::io::Error::from_raw_os_error(errno as i32)
}

struct InputThread {
    _handle: JoinHandle<()>,
}

impl InputThread {
    fn spawn(event_tx: mpsc::Sender<PtyHostEvent>) -> Self {
        let handle = std::thread::spawn(move || {
            let mut stdin = std::io::stdin().lock();
            let mut buf = [0u8; 1024];
            loop {
                match stdin.read(&mut buf) {
                    Ok(0) => {
                        let _ = event_tx.send(PtyHostEvent::Control(PtyControlEvent::Eof));
                        return;
                    }
                    Ok(n) => {
                        if event_tx
                            .send(PtyHostEvent::Input(buf[..n].to_vec()))
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(_) => {
                        let _ = event_tx.send(PtyHostEvent::Cancel);
                        return;
                    }
                }
            }
        });
        Self { _handle: handle }
    }
}

struct PtySignalForwarder {
    first_signal: Arc<AtomicI32>,
    handle: Handle,
    thread: Option<JoinHandle<()>>,
}

impl PtySignalForwarder {
    fn install(event_tx: mpsc::Sender<PtyHostEvent>) -> Result<Self, FcError> {
        let mut signals = Signals::new([SIGWINCH, SIGINT, SIGTERM, SIGHUP]).map_err(FcError::Io)?;
        let handle = signals.handle();
        let first_signal = Arc::new(AtomicI32::new(0));
        let first_signal_for_thread = Arc::clone(&first_signal);
        let thread = std::thread::spawn(move || {
            for signal in signals.forever() {
                forward_signal_event(
                    signal,
                    &event_tx,
                    &first_signal_for_thread,
                    current_pty_size,
                );
            }
        });
        Ok(Self {
            first_signal,
            handle,
            thread: Some(thread),
        })
    }

    fn observed_signal(&self) -> Option<i32> {
        match self.first_signal.load(Ordering::SeqCst) {
            0 => None,
            signal => Some(signal),
        }
    }
}

fn forward_signal_event(
    signal: i32,
    event_tx: &mpsc::Sender<PtyHostEvent>,
    first_signal: &AtomicI32,
    current_size: impl FnOnce() -> PtySize,
) {
    if signal == SIGWINCH {
        let _ = event_tx.send(PtyHostEvent::Resize(current_size()));
        return;
    }
    if first_signal
        .compare_exchange(0, signal, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        let _ = event_tx.send(PtyHostEvent::Cancel);
    }
}

impl Drop for PtySignalForwarder {
    fn drop(&mut self) {
        self.handle.close();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use m80_proto::{ExecStatus, ExecTiming};

    #[derive(Default)]
    struct FakeTerminalMode {
        enable_calls: usize,
        restore_calls: usize,
        fail_enable: bool,
    }

    impl TerminalMode for FakeTerminalMode {
        fn enable_raw(&mut self) -> std::io::Result<()> {
            self.enable_calls += 1;
            if self.fail_enable {
                Err(std::io::Error::other("raw mode failed"))
            } else {
                Ok(())
            }
        }

        fn restore(&mut self) -> std::io::Result<()> {
            self.restore_calls += 1;
            Ok(())
        }
    }

    #[test]
    fn raw_mode_guard_restores_on_child_exit_path() {
        let mut terminal = FakeTerminalMode::default();
        {
            let _guard = RawModeGuard::enter(&mut terminal).unwrap();
        }

        assert_eq!(terminal.enable_calls, 1);
        assert_eq!(terminal.restore_calls, 1);
    }

    #[test]
    fn raw_mode_guard_does_not_restore_when_enable_fails() {
        let mut terminal = FakeTerminalMode {
            fail_enable: true,
            ..FakeTerminalMode::default()
        };
        let err = match RawModeGuard::enter(&mut terminal) {
            Ok(_) => panic!("raw mode entry should fail"),
            Err(err) => err,
        };

        assert_eq!(err.kind(), std::io::ErrorKind::Other);
        assert_eq!(terminal.enable_calls, 1);
        assert_eq!(terminal.restore_calls, 0);
    }

    #[test]
    fn raw_mode_guard_restores_during_wrapper_error_unwind_path() {
        let mut terminal = FakeTerminalMode::default();
        let result = (|| -> Result<(), FcError> {
            let _guard = RawModeGuard::enter(&mut terminal).map_err(FcError::Io)?;
            Err(FcError::InvalidState {
                expected: "pty wrapper success",
                actual: "wrapper error",
            })
        })();

        assert!(matches!(result, Err(FcError::InvalidState { .. })));
        assert_eq!(terminal.restore_calls, 1);
    }

    #[test]
    fn copy_terminal_output_preserves_raw_bytes() {
        let mut stdout = Vec::new();
        copy_terminal_output(
            PtyOutputChunk {
                seq: 3,
                bytes: vec![0xff, b'p', b't', b'y'],
            },
            &mut stdout,
        )
        .unwrap();

        assert_eq!(stdout, vec![0xff, b'p', b't', b'y']);
    }

    #[test]
    fn pty_cancelled_exit_uses_conventional_signal_exit_code() {
        let exit = PtyExit {
            status: ExecStatus::Cancelled,
            exit_code: None,
            exit_signal: None,
            total_input_bytes: 0,
            total_output_bytes: 0,
            truncated: false,
            timing: ExecTiming {
                spawned_at_unix_ms: 0,
                exited_at_unix_ms: 0,
                spawn_ms: 0,
                run_ms: 0,
            },
        };

        assert_eq!(pty_exit_code(&exit, Some(SIGINT)), 130);
    }

    #[test]
    fn signal_forwarding_emits_resize_through_test_seam() {
        let (tx, rx) = mpsc::channel();
        let first_signal = AtomicI32::new(0);
        let size = PtySize {
            rows: 31,
            cols: 97,
            pixel_width: Some(800),
            pixel_height: Some(600),
        };

        forward_signal_event(SIGWINCH, &tx, &first_signal, || size);

        assert_eq!(rx.recv().unwrap(), PtyHostEvent::Resize(size));
        assert_eq!(first_signal.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn signal_forwarding_cancels_once_for_shutdown_signal() {
        let (tx, rx) = mpsc::channel();
        let first_signal = AtomicI32::new(0);

        forward_signal_event(SIGTERM, &tx, &first_signal, || unreachable!());
        forward_signal_event(SIGHUP, &tx, &first_signal, || unreachable!());

        assert_eq!(rx.recv().unwrap(), PtyHostEvent::Cancel);
        assert!(rx.try_recv().is_err());
        assert_eq!(first_signal.load(Ordering::SeqCst), SIGTERM);
    }
}
