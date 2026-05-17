//! Safe Linux guest-kernel syscall wrappers for m80-guestd.

use std::fs::File;
use std::io;
use std::os::fd::AsRawFd as _;

#[cfg(target_os = "linux")]
const RNDRESEEDCRNG: libc::c_ulong = 0x5207;

/// Fill `buf` completely using Linux `getrandom(2)`.
#[cfg(target_os = "linux")]
pub fn fill_random(mut buf: &mut [u8]) -> io::Result<()> {
    while !buf.is_empty() {
        // SAFETY: `buf.as_mut_ptr()` is valid for `buf.len()` writable bytes.
        // `getrandom` does not retain the pointer after returning.
        let n = unsafe { libc::getrandom(buf.as_mut_ptr().cast(), buf.len(), 0) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        let n = usize::try_from(n).map_err(|_| io::Error::other("negative getrandom count"))?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "getrandom returned zero bytes",
            ));
        }
        let (_, rest) = buf.split_at_mut(n);
        buf = rest;
    }
    Ok(())
}

/// Fill `buf` completely using Linux `getrandom(2)`.
#[cfg(not(target_os = "linux"))]
pub fn fill_random(_buf: &mut [u8]) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "getrandom wrapper is only supported on Linux",
    ))
}

/// Set the guest hostname.
#[cfg(target_os = "linux")]
pub fn set_hostname(name: &str) -> io::Result<()> {
    // SAFETY: `name.as_ptr()` is valid for `name.len()` readable bytes. Linux
    // `sethostname` copies exactly that byte range and does not require NUL.
    let result = unsafe { libc::sethostname(name.as_ptr().cast(), name.len()) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Set the guest hostname.
#[cfg(not(target_os = "linux"))]
pub fn set_hostname(_name: &str) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "sethostname wrapper is only supported on Linux",
    ))
}

/// Force a CRNG reseed through `ioctl(RNDRESEEDCRNG)`.
#[cfg(target_os = "linux")]
pub fn rndreseedcrng(file: &File) -> io::Result<()> {
    // SAFETY: `ioctl(RNDRESEEDCRNG)` consumes only the integer file descriptor
    // and no Rust pointers. The caller opened the fd for `/dev/urandom`.
    let result = unsafe { libc::ioctl(file.as_raw_fd(), RNDRESEEDCRNG as _) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Force a CRNG reseed through `ioctl(RNDRESEEDCRNG)`.
#[cfg(not(target_os = "linux"))]
pub fn rndreseedcrng(_file: &File) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "RNDRESEEDCRNG wrapper is only supported on Linux",
    ))
}
