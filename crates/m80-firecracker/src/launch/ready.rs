use std::io::Read;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::time::Duration;

use m80_proto::READY_PORT_DEFAULT;

use crate::error::{FcError, WireProtocolError};

/// Ready probe: total timeout.
///
/// Bounds how long phase_12b_ready_accept will wait for m80-guestd's
/// outbound connect to land. Has to cover guest kernel boot + (for
/// ubuntu) systemd reaching multi-user.target, with comfortable margin
/// for stress-loaded hosts.
const READY_TIMEOUT: Duration = Duration::from_secs(60);

/// Accept-loop sleep when the listener is non-blocking and no connection
/// has arrived yet. This is host-local (m80 polling its own UnixListener),
/// not interaction with Firecracker's muxer; no EAGAIN race possible.
const READY_ACCEPT_POLL: Duration = Duration::from_millis(10);

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
    vm_id: &str,
) -> Result<(), FcError> {
    accept_ready_signal(ready_listener, ready_path, READY_TIMEOUT)?;
    tracing::info!(vm_id, "ready signal received from guestd");
    Ok(())
}

pub(super) fn accept_ready_signal(
    ready_listener: &UnixListener,
    ready_path: &Path,
    timeout: Duration,
) -> Result<(), FcError> {
    ready_listener.set_nonblocking(true).map_err(FcError::Io)?;
    let deadline = std::time::Instant::now() + timeout;

    let mut stream = loop {
        // Check deadline before sleeping to avoid overshooting by one poll interval.
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(FcError::GuestdReadyTimeout {
                path: ready_path.to_path_buf(),
                timeout,
            });
        }
        match ready_listener.accept() {
            Ok((s, _)) => break s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(remaining.min(READY_ACCEPT_POLL));
            }
            Err(e) => return Err(FcError::Io(e)),
        }
    };

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
