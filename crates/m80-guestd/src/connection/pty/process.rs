//! PTY process and output-thread helpers.

use std::io::Read;
use std::sync::mpsc::SyncSender;
use std::thread::{self, JoinHandle};

use m80_proto::{CancelStatus, ExecStatus, PtyRequest, PtySignal as ProtoPtySignal, PtySize};
use portable_pty::{
    Child as PortableChild, CommandBuilder, ExitStatus as PortableExitStatus,
    PtySize as PortablePtySize,
};

use crate::{
    guest_log::{self, GuestLogPhase},
    workload_broker,
};

pub(super) use super::super::timeout_deadline;
use super::super::{
    cancel_status_from_group_signals, signal_process_group, PROCESS_GROUP_TERM_GRACE,
};

const OUTPUT_CHUNK_BYTES: usize = 4096;

pub(super) enum PtyFrame {
    Output { seq: u32, bytes: Vec<u8> },
}

pub(super) fn command_builder(req: &PtyRequest) -> anyhow::Result<CommandBuilder> {
    workload_broker::pty_command(req)
}

pub(super) fn spawn_output_thread(
    reader: Box<dyn Read + Send>,
    tx: SyncSender<PtyFrame>,
) -> JoinHandle<u64> {
    thread::spawn(move || capture_output(reader, tx))
}

fn capture_output(mut reader: Box<dyn Read + Send>, tx: SyncSender<PtyFrame>) -> u64 {
    let mut total = 0u64;
    let mut seq = 0u32;
    let mut tmp = [0u8; OUTPUT_CHUNK_BYTES];

    loop {
        match reader.read(&mut tmp) {
            Ok(0) => return total,
            Ok(n) => {
                total = total.saturating_add(n as u64);
                let bytes = tmp[..n].to_vec();
                let frame_seq = seq;
                seq = seq.wrapping_add(1);
                if tx
                    .send(PtyFrame::Output {
                        seq: frame_seq,
                        bytes,
                    })
                    .is_err()
                {
                    return total;
                }
            }
            Err(_) => return total,
        }
    }
}

pub(super) fn join_output_thread(output_thread: &mut Option<JoinHandle<u64>>) -> u64 {
    let Some(handle) = output_thread.take() else {
        return 0;
    };
    match handle.join() {
        Ok(total) => total,
        Err(_) => {
            guest_log::warn(
                GuestLogPhase::Exec,
                None,
                "pty output thread panicked; ending pty session",
            );
            0
        }
    }
}

pub(super) fn terminal_status(
    timed_out: bool,
    exit_status: Option<PortableExitStatus>,
) -> (ExecStatus, Option<i32>, Option<String>) {
    if timed_out {
        return (ExecStatus::TimedOut, None, None);
    }

    match exit_status {
        Some(status) if status.signal().is_none() => {
            (ExecStatus::Completed, Some(status.exit_code() as i32), None)
        }
        Some(status) => (ExecStatus::Failed, None, status.signal().map(str::to_owned)),
        None => (ExecStatus::Failed, None, None),
    }
}

pub(super) fn terminate_pty_child(
    child: &mut dyn PortableChild,
) -> (CancelStatus, Option<PortableExitStatus>) {
    let status = match child.process_id() {
        Some(raw_pid) => terminate_process_group(raw_pid),
        None => child
            .kill()
            .map(|()| CancelStatus::Cancelled)
            .unwrap_or(CancelStatus::Failed),
    };
    let exit_status = child.wait().ok();
    (status, exit_status)
}

fn terminate_process_group(raw_pid: u32) -> CancelStatus {
    let pgid = nix::unistd::Pid::from_raw(raw_pid as i32);
    let term = signal_process_group(pgid, nix::sys::signal::Signal::SIGTERM);
    thread::sleep(PROCESS_GROUP_TERM_GRACE);
    let kill = signal_process_group(pgid, nix::sys::signal::Signal::SIGKILL);
    cancel_status_from_group_signals(term, kill)
}

pub(super) fn signal_pty_child(
    child: &mut dyn PortableChild,
    signal: ProtoPtySignal,
) -> CancelStatus {
    let Some(raw_pid) = child.process_id() else {
        return CancelStatus::Failed;
    };
    let pgid = nix::unistd::Pid::from_raw(raw_pid as i32);
    cancel_status_from_single_signal(signal_process_group(pgid, to_nix_signal(signal)))
}

fn to_nix_signal(signal: ProtoPtySignal) -> nix::sys::signal::Signal {
    match signal {
        ProtoPtySignal::Interrupt => nix::sys::signal::Signal::SIGINT,
        ProtoPtySignal::Terminate => nix::sys::signal::Signal::SIGTERM,
        ProtoPtySignal::Hangup => nix::sys::signal::Signal::SIGHUP,
        ProtoPtySignal::Kill => nix::sys::signal::Signal::SIGKILL,
    }
}

fn cancel_status_from_single_signal(result: Result<(), nix::errno::Errno>) -> CancelStatus {
    match result {
        Ok(()) => CancelStatus::Cancelled,
        Err(nix::errno::Errno::ESRCH) => CancelStatus::AlreadyExited,
        Err(_) => CancelStatus::Failed,
    }
}

pub(super) fn to_portable_size(size: PtySize) -> PortablePtySize {
    PortablePtySize {
        rows: size.rows,
        cols: size.cols,
        pixel_width: size.pixel_width.unwrap_or(0),
        pixel_height: size.pixel_height.unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_output_thread_returns_zero_for_missing_thread() {
        let mut output_thread = None;
        assert_eq!(join_output_thread(&mut output_thread), 0);
    }

    #[test]
    fn join_output_thread_does_not_panic_on_worker_panic() {
        let mut output_thread = Some(thread::spawn(|| -> u64 {
            panic!("pty output worker panic");
        }));
        assert_eq!(join_output_thread(&mut output_thread), 0);
        assert!(output_thread.is_none());
    }
}
