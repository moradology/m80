//! Safe Linux `close_range(2)` wrapper for m80 process hardening.

use std::io;

/// Close every file descriptor from `min_fd` through `UINT_MAX`.
///
/// This is a direct `close_range(min_fd, UINT_MAX, 0)` call. It is intended for
/// single-purpose pre-exec hardening code where closing inherited descriptors is
/// the operation being requested.
#[cfg(target_os = "linux")]
pub fn close_from(min_fd: u32) -> io::Result<()> {
    // SAFETY: `close_range` takes integer file descriptor bounds and does not
    // dereference Rust pointers. Any impact on open descriptors is the explicit
    // process-level side effect requested by this wrapper.
    let result = unsafe { libc::syscall(libc::SYS_close_range, min_fd, u32::MAX, 0_u32) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Close every file descriptor from `min_fd` through `UINT_MAX`.
#[cfg(not(target_os = "linux"))]
pub fn close_from(_min_fd: u32) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "close_range is only supported on Linux",
    ))
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use std::fs::File;
    #[cfg(target_os = "linux")]
    use std::os::fd::AsRawFd;

    #[cfg(target_os = "linux")]
    fn fd_is_open(fd: libc::c_int) -> bool {
        // SAFETY: `fcntl(F_GETFD)` only reads descriptor flags for an integer fd.
        unsafe { libc::fcntl(fd, libc::F_GETFD) >= 0 }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn close_from_closes_the_lower_bound_fd_in_child() {
        let file = File::open("/dev/null").expect("open /dev/null");
        // SAFETY: `F_DUPFD_CLOEXEC` duplicates a valid fd at or above the given
        // lower bound; the returned fd is owned by this test until closed.
        let fd = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
        assert!(fd >= 3, "duplicate fd: {fd}");
        assert!(fd_is_open(fd));

        // SAFETY: the child does not return into the Rust test harness; it only
        // performs the close-range probe, checks the duplicated fd, and exits.
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0, "fork failed: {}", std::io::Error::last_os_error());
        if pid == 0 {
            let close_ok = super::close_from(3).is_ok();
            let fd_closed = !fd_is_open(fd);
            // SAFETY: `_exit` terminates the forked child without running Rust
            // destructors after close_range has closed inherited fds.
            unsafe { libc::_exit(if close_ok && fd_closed { 0 } else { 1 }) };
        }

        let mut status = 0;
        // SAFETY: `pid` is the child returned by `fork`; `status` is a valid
        // out-pointer for wait status.
        let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
        assert_eq!(waited, pid, "waitpid failed");
        // SAFETY: close the duplicated fd that remained open in the parent.
        unsafe {
            libc::close(fd);
        }

        assert!(libc::WIFEXITED(status), "child did not exit cleanly");
        assert_eq!(libc::WEXITSTATUS(status), 0, "child close_range probe");
    }
}
