//! Background warm-pool refill worker.

use std::sync::Arc;

use super::{discard_sandbox, WarmPoolInner, FILL_BACKOFF};

pub(super) fn spawn_fill_worker(inner: Arc<WarmPoolInner>, cpuset_cpus: Option<String>) {
    // Guard rolls back `filling` if thread::spawn panics (e.g. under
    // resource exhaustion). The thread closure disarms it immediately on
    // entry, taking over decrement responsibility via its match arms.
    struct FillGuard {
        inner: Option<Arc<WarmPoolInner>>,
        cpuset_cpus: Option<String>,
    }
    impl FillGuard {
        fn disarm(&mut self) {
            self.inner = None;
            self.cpuset_cpus = None;
        }
    }
    impl Drop for FillGuard {
        fn drop(&mut self) {
            if let Some(inner) = self.inner.take() {
                let mut state = inner.state.lock().unwrap_or_else(|p| p.into_inner());
                state.filling = state.filling.saturating_sub(1);
                state.release_cpuset_cpus(self.cpuset_cpus.take());
                inner.changed.notify_all();
            }
        }
    }
    let mut guard = FillGuard {
        inner: Some(Arc::clone(&inner)),
        cpuset_cpus: cpuset_cpus.clone(),
    };
    std::thread::spawn(move || {
        guard.disarm();
        let launched = inner.launch_slot(cpuset_cpus.clone());
        let backoff = match launched {
            Ok(slot) if inner.shutdown.load(std::sync::atomic::Ordering::Relaxed) => {
                let _ = discard_sandbox(slot.sandbox);
                let mut state = inner.state.lock().unwrap_or_else(|p| p.into_inner());
                state.filling = state.filling.saturating_sub(1);
                state.release_cpuset_cpus(slot.cpuset_cpus);
                inner.changed.notify_all();
                return;
            }
            Ok(slot) => {
                let mut state = inner.state.lock().unwrap_or_else(|p| p.into_inner());
                state.filling = state.filling.saturating_sub(1);
                state.ready.push_back(slot);
                state.last_fill_error = None;
                state.consecutive_fill_errors = 0;
                inner.changed.notify_all();
                None
            }
            Err(e) => {
                let mut state = inner.state.lock().unwrap_or_else(|p| p.into_inner());
                state.filling = state.filling.saturating_sub(1);
                state.release_cpuset_cpus(cpuset_cpus);
                state.consecutive_fill_errors = state.consecutive_fill_errors.saturating_add(1);
                let idx = (state.consecutive_fill_errors as usize - 1).min(FILL_BACKOFF.len() - 1);
                let delay = FILL_BACKOFF[idx];
                state.last_fill_error = Some(e.to_string());
                inner.changed.notify_all();
                Some(delay)
            }
        };
        if let Some(delay) = backoff {
            std::thread::sleep(delay);
            inner.start_background_fill();
        }
    });
}
