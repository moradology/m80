//! Shared workload spawn boundary for guest exec paths.

use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Context as _;
use m80_proto::{ExecRequest, PtyRequest};
use portable_pty::CommandBuilder;

use crate::exec_sandbox;

const BROKER_REQUEST_LIMIT: usize = 1 << 20;

static WORKLOAD_BROKER_POISONED: AtomicBool = AtomicBool::new(false);

/// Workload launch surface routed through the broker seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkloadKind {
    /// Buffered `ExecRequest`.
    BufferedExec,
    /// Streaming `ExecRequest`.
    StreamingExec,
    /// PTY request.
    PtyExec,
}

/// Mark the broker unavailable. Later workload spawn requests fail closed.
#[allow(dead_code)]
pub(crate) fn poison_workload_broker() {
    WORKLOAD_BROKER_POISONED.store(true, Ordering::Release);
}

/// Build a stdio-piped command for buffered or streaming exec.
pub(crate) fn exec_command(req: &ExecRequest, kind: WorkloadKind) -> anyhow::Result<Command> {
    debug_assert!(matches!(
        kind,
        WorkloadKind::BufferedExec | WorkloadKind::StreamingExec
    ));
    ensure_broker_available()?;
    validate_exec_request_size(req)?;
    record_spawn(kind);

    let (program, args) = exec_sandbox::command_program_and_args(&req.program, &req.args);
    let mut cmd = Command::new(program);
    cmd.args(args);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.process_group(0);
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
    Ok(cmd)
}

/// Build a PTY command for the shared broker seam.
pub(crate) fn pty_command(req: &PtyRequest) -> anyhow::Result<CommandBuilder> {
    ensure_broker_available()?;
    validate_pty_request_size(req)?;
    record_spawn(WorkloadKind::PtyExec);

    let (program, args) = exec_sandbox::command_program_and_args(&req.program, &req.args);
    let mut cmd = CommandBuilder::new(program);
    cmd.args(args);
    cmd.env_clear();

    if let Some(env_pairs) = &req.env {
        for (k, v) in env_pairs {
            cmd.env(k, v);
        }
    } else {
        for (k, v) in std::env::vars_os() {
            cmd.env(k, v);
        }
    }

    let cwd: OsString = match &req.cwd {
        Some(cwd) => cwd.into(),
        None => std::env::current_dir()
            .context("read current directory for pty workload")?
            .into_os_string(),
    };
    cmd.cwd(cwd);
    Ok(cmd)
}

fn ensure_broker_available() -> anyhow::Result<()> {
    if WORKLOAD_BROKER_POISONED.load(Ordering::Acquire) {
        anyhow::bail!("workload broker unavailable");
    }
    Ok(())
}

fn validate_exec_request_size(req: &ExecRequest) -> anyhow::Result<()> {
    let size = string_size(&req.program)
        + strings_size(&req.args)
        + opt_string_size(req.cwd.as_ref())
        + env_size(req.env.as_ref())
        + req.stdin.as_ref().map(Vec::len).unwrap_or(0);
    validate_broker_size(size)
}

fn validate_pty_request_size(req: &PtyRequest) -> anyhow::Result<()> {
    let size = string_size(&req.program)
        + strings_size(&req.args)
        + opt_string_size(req.cwd.as_ref())
        + env_size(req.env.as_ref());
    validate_broker_size(size)
}

fn validate_broker_size(size: usize) -> anyhow::Result<()> {
    if size > BROKER_REQUEST_LIMIT {
        anyhow::bail!(
            "workload broker request too large: {size} bytes exceeds limit {BROKER_REQUEST_LIMIT}"
        );
    }
    Ok(())
}

fn string_size(value: &str) -> usize {
    value.len()
}

fn opt_string_size(value: Option<&String>) -> usize {
    value.map(|s| s.len()).unwrap_or(0)
}

fn strings_size(values: &[String]) -> usize {
    values.iter().map(String::len).sum()
}

fn env_size(env: Option<&Vec<(String, String)>>) -> usize {
    env.map(|pairs| {
        pairs
            .iter()
            .map(|(key, value)| key.len() + value.len())
            .sum()
    })
    .unwrap_or(0)
}

fn record_spawn(_kind: WorkloadKind) {
    #[cfg(test)]
    test_hook::record(_kind);
}

#[cfg(test)]
mod test_hook {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Mutex, MutexGuard};

    use super::WorkloadKind;

    static OBSERVED: AtomicUsize = AtomicUsize::new(0);
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    pub(super) fn record(kind: WorkloadKind) {
        OBSERVED.fetch_or(bit(kind), Ordering::AcqRel);
    }

    pub(super) fn observed(kind: WorkloadKind) -> bool {
        OBSERVED.load(Ordering::Acquire) & bit(kind) != 0
    }

    pub(super) fn lock() -> MutexGuard<'static, ()> {
        TEST_LOCK
            .lock()
            .expect("workload broker test lock poisoned")
    }

    pub(super) fn reset() {
        OBSERVED.store(0, Ordering::Release);
        super::WORKLOAD_BROKER_POISONED.store(false, std::sync::atomic::Ordering::Release);
    }

    fn bit(kind: WorkloadKind) -> usize {
        match kind {
            WorkloadKind::BufferedExec => 1 << 0,
            WorkloadKind::StreamingExec => 1 << 1,
            WorkloadKind::PtyExec => 1 << 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exec_req() -> ExecRequest {
        ExecRequest {
            program: "/bin/true".into(),
            args: Vec::new(),
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(1_000),
            streaming: false,
        }
    }

    fn pty_req() -> PtyRequest {
        PtyRequest {
            program: "/bin/true".into(),
            args: Vec::new(),
            cwd: None,
            env: None,
            timeout_ms: Some(1_000),
            size: m80_proto::PtySize {
                rows: 24,
                cols: 80,
                pixel_width: None,
                pixel_height: None,
            },
        }
    }

    #[test]
    fn buffered_exec_routes_through_broker_seam() {
        let _guard = test_hook::lock();
        test_hook::reset();

        let _ = exec_command(&exec_req(), WorkloadKind::BufferedExec).unwrap();

        assert!(test_hook::observed(WorkloadKind::BufferedExec));
    }

    #[test]
    fn streaming_exec_routes_through_broker_seam() {
        let _guard = test_hook::lock();
        test_hook::reset();

        let _ = exec_command(&exec_req(), WorkloadKind::StreamingExec).unwrap();

        assert!(test_hook::observed(WorkloadKind::StreamingExec));
    }

    #[test]
    fn pty_exec_routes_through_broker_seam() {
        let _guard = test_hook::lock();
        test_hook::reset();

        let _ = pty_command(&pty_req()).unwrap();

        assert!(test_hook::observed(WorkloadKind::PtyExec));
    }

    #[test]
    fn poisoned_broker_fails_closed() {
        let _guard = test_hook::lock();
        test_hook::reset();
        poison_workload_broker();

        let err = exec_command(&exec_req(), WorkloadKind::BufferedExec).unwrap_err();

        assert!(err.to_string().contains("workload broker unavailable"));
    }

    #[test]
    fn oversized_broker_request_fails_before_spawn() {
        let _guard = test_hook::lock();
        test_hook::reset();
        let mut req = exec_req();
        req.args.push("x".repeat(BROKER_REQUEST_LIMIT + 1));

        let err = exec_command(&req, WorkloadKind::BufferedExec).unwrap_err();

        assert!(err
            .to_string()
            .contains("workload broker request too large"));
    }
}
