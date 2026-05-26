//! Deadline-bounded teardown helpers.

use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use m80_jailer::MaterializedJail;

use crate::error::{CleanupDeadlinePhase, FcError};
use crate::panic_payload;

pub(crate) const WATCHER_JOIN_TIMEOUT: Duration = Duration::from_secs(2);
const JAIL_DROP_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const RUN_DIR_DELETE_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) fn join_watcher_with_timeout(vm_id: &str, handle: JoinHandle<()>) {
    match join_thread_with_timeout(handle, WATCHER_JOIN_TIMEOUT) {
        JoinOutcome::Joined => {}
        JoinOutcome::Panicked(panic) => {
            tracing::error!(
                vm_id = %vm_id,
                panic = %panic,
                "lifecycle watcher thread panicked during teardown"
            );
        }
        JoinOutcome::TimedOut => {
            tracing::error!(
                vm_id,
                timeout = ?WATCHER_JOIN_TIMEOUT,
                "lifecycle watcher thread did not stop within timeout; detaching join waiter"
            );
        }
    }
}

pub(crate) fn drop_jail_with_timeout(vm_id: &str, jail: MaterializedJail) {
    if let Err(err) = run_cleanup_with_timeout(
        vm_id,
        CleanupDeadlinePhase::JailDrop,
        JAIL_DROP_TIMEOUT,
        move || {
            drop(jail);
            Ok(())
        },
    ) {
        tracing::error!(
            vm_id,
            error = %err,
            "materialized jail cleanup did not complete before deadline"
        );
    }
}

pub(crate) fn run_cleanup_with_timeout<F>(
    vm_id: &str,
    phase: CleanupDeadlinePhase,
    timeout: Duration,
    cleanup: F,
) -> Result<(), FcError>
where
    F: FnOnce() -> Result<(), FcError> + Send + 'static,
{
    let (done_tx, done_rx) = mpsc::channel();
    thread::spawn(move || {
        let result = cleanup();
        let _ = done_tx.send(result);
    });

    match done_rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(FcError::CleanupDeadlineExceeded {
            vm_id: vm_id.to_owned(),
            phase,
            timeout,
        }),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(FcError::InvalidState {
            expected: "cleanup thread completion",
            actual: "cleanup thread disconnected",
        }),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum JoinOutcome {
    Joined,
    Panicked(String),
    TimedOut,
}

fn join_thread_with_timeout(handle: JoinHandle<()>, timeout: Duration) -> JoinOutcome {
    let (done_tx, done_rx) = mpsc::channel();
    thread::spawn(move || {
        let outcome = match handle.join() {
            Ok(()) => JoinOutcome::Joined,
            Err(payload) => JoinOutcome::Panicked(panic_payload::describe(payload.as_ref())),
        };
        let _ = done_tx.send(outcome);
    });

    done_rx
        .recv_timeout(timeout)
        .unwrap_or(JoinOutcome::TimedOut)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Instant;

    #[test]
    fn watcher_join_panic_reports_payload() {
        let handle = thread::spawn(|| {
            panic!("idle watcher payload");
        });

        let outcome = join_thread_with_timeout(handle, Duration::from_secs(1));

        assert_eq!(
            outcome,
            JoinOutcome::Panicked("idle watcher payload".to_owned())
        );
    }

    #[test]
    fn watcher_join_timeout_returns_without_waiting_for_thread_exit() {
        let (release_tx, release_rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let _ = release_rx.recv();
        });

        let started = Instant::now();
        let outcome = join_thread_with_timeout(handle, Duration::from_millis(10));
        let elapsed = started.elapsed();
        release_tx.send(()).expect("release blocked watcher");

        assert_eq!(outcome, JoinOutcome::TimedOut);
        assert!(
            elapsed < Duration::from_millis(100),
            "join timeout should detach promptly, elapsed={elapsed:?}"
        );
    }

    #[test]
    fn cleanup_timeout_returns_without_waiting_for_operation_exit() {
        let (release_tx, release_rx) = mpsc::channel();

        let started = Instant::now();
        let err = run_cleanup_with_timeout(
            "vm-cleanup-timeout",
            CleanupDeadlinePhase::RunDirDelete,
            Duration::from_millis(10),
            move || {
                let _ = release_rx.recv();
                Ok(())
            },
        )
        .expect_err("blocked cleanup should time out");
        let elapsed = started.elapsed();
        release_tx.send(()).expect("release blocked cleanup");

        assert!(
            matches!(
                err,
                FcError::CleanupDeadlineExceeded {
                    ref vm_id,
                    phase: CleanupDeadlinePhase::RunDirDelete,
                    timeout,
                } if vm_id == "vm-cleanup-timeout" && timeout == Duration::from_millis(10)
            ),
            "unexpected timeout error: {err:?}"
        );
        assert!(
            elapsed < Duration::from_millis(100),
            "cleanup timeout should return promptly, elapsed={elapsed:?}"
        );
    }
}
