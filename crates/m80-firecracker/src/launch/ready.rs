use std::io::Read;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::prelude::AsFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use m80_proto::READY_PORT_DEFAULT;
use nix::errno::Errno;
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};

use crate::error::{FcError, WireProtocolError};

/// Ready probe: total timeout.
///
/// Bounds how long phase_12b_ready_accept will wait for m80-guestd's
/// outbound connect to land. Has to cover guest kernel boot + (for
/// ubuntu) systemd reaching multi-user.target, with comfortable margin
/// for stress-loaded hosts.
const READY_TIMEOUT: Duration = Duration::from_secs(60);

/// Read-deadline for the proto-version byte after `accept()`.
const READY_VERSION_READ_TIMEOUT: Duration = Duration::from_secs(2);

/// Bind the inverted-readiness `UnixListener` at `<vsock_uds>_<READY_PORT>`.
///
/// Firecracker's muxer follows the `<host_sock_path>_<port>` convention
/// when the guest does an outbound vsock connect: it `UnixStream::connect()`s
/// to that path. We bind the listener BEFORE InstanceStart so the muxer
/// finds it ready when guestd's outbound connect lands.
///
/// Permissions: the file is created with the m80 process's umask. Since
/// Firecracker, running as `jail_uid` post-jailer-launch, needs to
/// `connect(2)` to the socket, which requires write permission on the
/// socket file, we explicitly chown to the jail uid.
pub(super) fn phase_11b_bind_ready_listener(
    path: &Path,
    jail_uid: u32,
) -> Result<UnixListener, FcError> {
    if path.exists() {
        // Stale from a previous launch with the same vm_id. Remove so
        // bind() doesn't fail with EADDRINUSE.
        let _ = std::fs::remove_file(path);
    }
    let listener = UnixListener::bind(path).map_err(FcError::Io)?;

    // Make the socket connect()-able by the jail uid. Both the inode
    // ownership (chown) and the directory's access bits matter; the
    // jailer materialize step already produces a jail dir owned by
    // `jail_uid`, so `chown` on the socket file alone is sufficient.
    use nix::unistd::{chown, Uid};
    chown(path, Some(Uid::from_raw(jail_uid)), None)
        .map_err(|e| FcError::Io(std::io::Error::from_raw_os_error(e as i32)))?;

    Ok(listener)
}

pub(super) fn ready_listener_path(vsock_uds: &Path) -> PathBuf {
    let mut path = vsock_uds.as_os_str().to_os_string();
    path.push(format!("_{READY_PORT_DEFAULT}"));
    PathBuf::from(path)
}

/// `accept()` the inverted-readiness signal from m80-guestd and validate the
/// protocol-version byte.
///
/// The host-local `accept()` poll-loop here is not a muxer-polling race.
/// We're polling our own UnixListener; the muxer only fires once when guestd
/// does its outbound connect.
pub(super) fn phase_12b_ready_accept(
    ready_listener: &UnixListener,
    ready_path: &Path,
    _vsock_uds: &Path,
    console_log: &Path,
    vm_id: &str,
) -> Result<(), FcError> {
    let wait_started = Instant::now();
    accept_ready_signal(ready_listener, ready_path, READY_TIMEOUT)?;
    crate::diagnostics::phase_event(
        "phase_12b_host_waiting_accept",
        vm_id,
        wait_started.elapsed(),
    );
    emit_guest_boot_phase_events(console_log, vm_id);
    tracing::info!(vm_id, "ready signal received from guestd");
    Ok(())
}

fn emit_guest_boot_phase_events(console_log: &Path, vm_id: &str) {
    let Ok(text) = std::fs::read_to_string(console_log) else {
        tracing::debug!(
            vm_id,
            path = %console_log.display(),
            "ready accept: console log not readable for guest boot phase parse"
        );
        return;
    };
    if let Some(range_us) = kernel_console_timestamp_range_us(&text) {
        crate::diagnostics::phase_event(
            "phase_12b_kernel_console_range",
            vm_id,
            Duration::from_micros(range_us),
        );
    }
    for event in guest_boot_phase_events_from_console_text(&text) {
        crate::diagnostics::phase_event(
            &event.phase_name,
            vm_id,
            Duration::from_micros(event.elapsed_us),
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GuestBootPhaseEvent {
    pub(super) phase_name: String,
    pub(super) elapsed_us: u64,
}

pub(super) fn guest_boot_phase_events_from_console_text(text: &str) -> Vec<GuestBootPhaseEvent> {
    text.lines()
        .filter_map(guest_boot_phase_event_from_line)
        .collect()
}

pub(super) fn kernel_console_timestamp_range_us(text: &str) -> Option<u64> {
    let mut first = None;
    let mut last = None;
    for us in text
        .lines()
        .filter_map(kernel_console_timestamp_us_from_line)
    {
        first.get_or_insert(us);
        last = Some(us);
    }
    Some(last?.saturating_sub(first?))
}

fn kernel_console_timestamp_us_from_line(line: &str) -> Option<u64> {
    let start = line.find('[')?;
    let rest = &line[start + 1..];
    let end = rest.find(']')?;
    let timestamp = rest[..end].trim();
    let (seconds, micros) = timestamp.split_once('.')?;
    let seconds = seconds.trim().parse::<u64>().ok()?;
    let micros = micros.trim();
    let mut frac = micros.chars().take(6).collect::<String>();
    while frac.len() < 6 {
        frac.push('0');
    }
    Some(seconds.saturating_mul(1_000_000) + frac.parse::<u64>().ok()?)
}

fn guest_boot_phase_event_from_line(line: &str) -> Option<GuestBootPhaseEvent> {
    let payload = line.strip_prefix("M80_GUEST_BOOT ")?;
    let mut name = None;
    let mut elapsed_us = None;
    for token in payload.split_whitespace() {
        if let Some(value) = token.strip_prefix("name=") {
            name = Some(value);
        } else if let Some(value) = token.strip_prefix("elapsed_us=") {
            elapsed_us = value.parse::<u64>().ok();
        }
    }
    Some(GuestBootPhaseEvent {
        phase_name: format!("phase_12b_guest_{}", name?),
        elapsed_us: elapsed_us?,
    })
}

pub(super) fn accept_ready_signal(
    ready_listener: &UnixListener,
    ready_path: &Path,
    timeout: Duration,
) -> Result<(), FcError> {
    ready_listener.set_nonblocking(true).map_err(FcError::Io)?;
    let deadline = Instant::now() + timeout;
    let mut stream = accept_ready_connection(ready_listener, ready_path, deadline, timeout)?;

    stream.set_nonblocking(false).map_err(FcError::Io)?;
    stream
        .set_read_timeout(Some(READY_VERSION_READ_TIMEOUT))
        .map_err(FcError::Io)?;
    let mut buf = [0u8; 1];
    stream.read_exact(&mut buf).map_err(FcError::Io)?;
    if buf[0] != m80_proto::PROTOCOL_VERSION as u8 {
        return Err(FcError::Protocol(WireProtocolError::UnsupportedVersion {
            expected: m80_proto::PROTOCOL_VERSION,
            got: u32::from(buf[0]),
        }));
    }
    drop(stream);
    Ok(())
}

fn accept_ready_connection(
    ready_listener: &UnixListener,
    ready_path: &Path,
    deadline: Instant,
    timeout: Duration,
) -> Result<UnixStream, FcError> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(FcError::GuestdReadyTimeout {
                path: ready_path.to_path_buf(),
                timeout,
            });
        }

        let mut fds = [PollFd::new(ready_listener.as_fd(), PollFlags::POLLIN)];
        match poll(&mut fds, poll_timeout_for_duration(remaining)?) {
            Ok(0) => {
                return Err(FcError::GuestdReadyTimeout {
                    path: ready_path.to_path_buf(),
                    timeout,
                });
            }
            Ok(_) => match ready_listener.accept() {
                Ok((stream, _)) => return Ok(stream),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(e) => return Err(FcError::Io(e)),
            },
            Err(Errno::EINTR) => continue,
            Err(errno) => {
                return Err(FcError::Io(std::io::Error::from_raw_os_error(errno as i32)));
            }
        }
    }
}

fn poll_timeout_for_duration(duration: Duration) -> Result<PollTimeout, FcError> {
    PollTimeout::try_from(duration).map_err(|e| {
        FcError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("poll timeout out of range: {e}"),
        ))
    })
}
