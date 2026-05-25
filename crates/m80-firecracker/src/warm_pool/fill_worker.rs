//! Background warm-pool refill worker.

use std::sync::Arc;
use std::time::Instant;

use super::{discard_sandbox, duration_micros_u64, WarmPoolInner, FILL_BACKOFF};

pub(super) fn spawn_fill_worker(inner: Arc<WarmPoolInner>, cpuset_cpus: Option<String>) {
    // One guard covers thread::spawn panics before the closure starts; the
    // closure installs its own guard so a panic in launch_slot cannot strand
    // `filling > 0` and hang WarmPool::Drop.
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
                state.record_fill_failure(
                    "fill worker panicked before completing slot launch".into(),
                );
                inner.changed.notify_all();
            }
        }
    }
    let mut spawn_guard = FillGuard {
        inner: Some(Arc::clone(&inner)),
        cpuset_cpus: cpuset_cpus.clone(),
    };
    std::thread::spawn(move || {
        let mut fill_guard = FillGuard {
            inner: Some(Arc::clone(&inner)),
            cpuset_cpus: cpuset_cpus.clone(),
        };
        {
            let mut state = inner.state.lock().unwrap_or_else(|p| p.into_inner());
            state.record_fill_attempt();
        }
        let fill_started = Instant::now();
        let launched = inner.launch_slot(cpuset_cpus.clone());
        let fill_duration_us = duration_micros_u64(fill_started.elapsed());
        let backoff = match launched {
            Ok(slot) if inner.shutdown.load(std::sync::atomic::Ordering::Acquire) => {
                let discard_result = discard_sandbox(slot.sandbox);
                let mut state = inner.state.lock().unwrap_or_else(|p| p.into_inner());
                state.filling = state.filling.saturating_sub(1);
                state.release_cpuset_cpus(slot.cpuset_cpus);
                state.discarded = state.discarded.saturating_add(1);
                state.record_fill_success(fill_duration_us);
                inner.changed.notify_all();
                fill_guard.disarm();
                if let Err(err) = discard_result {
                    tracing::error!(error = %err, "failed to discard warm-pool slot after shutdown");
                }
                return;
            }
            Ok(slot) => {
                let mut state = inner.state.lock().unwrap_or_else(|p| p.into_inner());
                state.filling = state.filling.saturating_sub(1);
                state.ready.push_back(slot);
                state.record_fill_success(fill_duration_us);
                inner.changed.notify_all();
                None
            }
            Err(e) => {
                let mut state = inner.state.lock().unwrap_or_else(|p| p.into_inner());
                state.filling = state.filling.saturating_sub(1);
                state.release_cpuset_cpus(cpuset_cpus);
                state.record_fill_failure(e.to_string());
                let idx = (state.consecutive_fill_errors as usize - 1).min(FILL_BACKOFF.len() - 1);
                let delay = FILL_BACKOFF[idx];
                inner.changed.notify_all();
                Some(delay)
            }
        };
        fill_guard.disarm();
        if let Some(delay) = backoff {
            std::thread::sleep(delay);
            inner.start_background_fill();
        }
    });
    spawn_guard.disarm();
}
