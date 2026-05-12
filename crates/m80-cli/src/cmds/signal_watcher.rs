use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use signal_hook::iterator::Handle;

/// Shared infrastructure for signal-watching threads.
///
/// Both `SignalCancellation` (run_stream) and `PtySignalForwarder` (pty) hold
/// identical fields, the same `observed_signal` accessor, and the same `Drop`
/// impl.  Only the thread body differs.  This type captures the common skeleton;
/// each caller constructs it by spawning its own thread and passing the handle.
pub(super) struct SignalWatcher {
    pub(super) first_signal: Arc<AtomicI32>,
    handle: Handle,
    thread: Option<JoinHandle<()>>,
}

impl SignalWatcher {
    pub(super) fn new(
        first_signal: Arc<AtomicI32>,
        handle: Handle,
        thread: JoinHandle<()>,
    ) -> Self {
        Self {
            first_signal,
            handle,
            thread: Some(thread),
        }
    }

    pub(super) fn observed_signal(&self) -> Option<i32> {
        match self.first_signal.load(Ordering::SeqCst) {
            0 => None,
            signal => Some(signal),
        }
    }
}

impl Drop for SignalWatcher {
    fn drop(&mut self) {
        self.handle.close();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
