//! Process-hardening wrapper used before execing Firecracker's official jailer.

use std::ffi::{OsStr, OsString};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use caps::CapSet;
use nix::errno::Errno;
use nix::sys::prctl;
use nix::sys::signal::{SigSet, SigmaskHow, Signal};
use nix::sys::stat::{umask, Mode};
use nix::unistd::{close, setgroups};

/// Parsed command-line input for `m80-jailer-harden`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardenArgs {
    /// Official Firecracker jailer binary to exec after hardening.
    pub jailer_bin: PathBuf,
    /// Jailed uid, carried for diagnostics and future proof checks.
    pub uid: u32,
    /// Jailed gid, carried for diagnostics and future proof checks.
    pub gid: u32,
    /// Arguments forwarded to the official jailer.
    pub jailer_args: Vec<OsString>,
}

/// Errors returned by the wrapper before it successfully replaces itself.
#[derive(Debug, thiserror::Error)]
pub enum HardenError {
    /// Required argument was absent.
    #[error("missing required argument: {0}")]
    MissingArgument(&'static str),
    /// Argument value could not be parsed.
    #[error("invalid {field}: {value}")]
    InvalidValue {
        /// Field name.
        field: &'static str,
        /// Supplied value.
        value: String,
    },
    /// The `--` separator before jailer args was absent.
    #[error("missing -- separator before jailer args")]
    MissingSeparator,
    /// No jailer arguments were supplied after `--`.
    #[error("missing jailer args after --")]
    MissingJailerArgs,
    /// Supplementary groups could not be cleared.
    #[error("setgroups([]): {0}")]
    SetGroups(#[source] nix::Error),
    /// Capability set could not be cleared.
    #[error("clear {set} capabilities: {source}")]
    ClearCaps {
        /// Capability set name.
        set: &'static str,
        /// Source error.
        source: caps::errors::CapsError,
    },
    /// `PR_SET_NO_NEW_PRIVS` failed.
    #[error("set no_new_privs: {0}")]
    NoNewPrivs(#[source] nix::Error),
    /// `PR_SET_PDEATHSIG` failed.
    #[error("set pdeathsig: {0}")]
    ParentDeathSignal(#[source] nix::Error),
    /// Signal mask reset failed.
    #[error("reset signal mask: {0}")]
    SignalMask(#[source] nix::Error),
    /// Inherited file descriptors could not be enumerated.
    #[error("enumerate inherited fds: {0}")]
    EnumerateFds(#[source] std::io::Error),
    /// Inherited file descriptor could not be closed.
    #[error("close inherited fd {fd}: {source}")]
    CloseFd {
        /// File descriptor number.
        fd: i32,
        /// Source error.
        source: nix::Error,
    },
    /// Final exec failed.
    #[error("exec {}: {source}", path.display())]
    Exec {
        /// Binary path.
        path: PathBuf,
        /// Source error.
        source: std::io::Error,
    },
}

/// Parse wrapper arguments from an iterator whose first item is already the
/// first real argument, not argv[0].
pub fn parse_args<I, S>(args: I) -> Result<HardenArgs, HardenError>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut iter = args.into_iter().map(Into::into);
    let mut jailer_bin = None;
    let mut uid = None;
    let mut gid = None;

    while let Some(arg) = iter.next() {
        if arg == OsStr::new("--") {
            let jailer_args: Vec<_> = iter.collect();
            if jailer_args.is_empty() {
                return Err(HardenError::MissingJailerArgs);
            }
            return Ok(HardenArgs {
                jailer_bin: jailer_bin.ok_or(HardenError::MissingArgument("--jailer-bin"))?,
                uid: uid.ok_or(HardenError::MissingArgument("--uid"))?,
                gid: gid.ok_or(HardenError::MissingArgument("--gid"))?,
                jailer_args,
            });
        }

        match arg.to_string_lossy().as_ref() {
            "--jailer-bin" => {
                jailer_bin = Some(PathBuf::from(
                    iter.next()
                        .ok_or(HardenError::MissingArgument("--jailer-bin"))?,
                ));
            }
            "--uid" => {
                uid = Some(parse_u32(
                    "--uid",
                    iter.next().ok_or(HardenError::MissingArgument("--uid"))?,
                )?);
            }
            "--gid" => {
                gid = Some(parse_u32(
                    "--gid",
                    iter.next().ok_or(HardenError::MissingArgument("--gid"))?,
                )?);
            }
            other => {
                return Err(HardenError::InvalidValue {
                    field: "argument",
                    value: other.to_owned(),
                });
            }
        }
    }

    Err(HardenError::MissingSeparator)
}

fn parse_u32(field: &'static str, value: OsString) -> Result<u32, HardenError> {
    let value = value.to_string_lossy().into_owned();
    value
        .parse()
        .map_err(|_| HardenError::InvalidValue { field, value })
}

/// Apply inherited one-way process hardening before execing the official jailer.
pub fn apply_process_hardening() -> Result<(), HardenError> {
    setgroups(&[]).map_err(HardenError::SetGroups)?;
    caps::clear(None, CapSet::Inheritable).map_err(|source| HardenError::ClearCaps {
        set: "inheritable",
        source,
    })?;
    caps::clear(None, CapSet::Ambient).map_err(|source| HardenError::ClearCaps {
        set: "ambient",
        source,
    })?;
    prctl::set_no_new_privs().map_err(HardenError::NoNewPrivs)?;
    prctl::set_pdeathsig(Signal::SIGKILL).map_err(HardenError::ParentDeathSignal)?;
    umask(Mode::from_bits_truncate(0o077));
    let empty = SigSet::empty();
    nix::sys::signal::pthread_sigmask(SigmaskHow::SIG_SETMASK, Some(&empty), None)
        .map_err(HardenError::SignalMask)?;
    close_inherited_fds()?;
    Ok(())
}

fn close_inherited_fds() -> Result<(), HardenError> {
    let mut fds = Vec::new();
    for entry in std::fs::read_dir("/proc/self/fd").map_err(HardenError::EnumerateFds)? {
        let entry = entry.map_err(HardenError::EnumerateFds)?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Ok(fd) = name.parse::<i32>() else {
            continue;
        };
        if fd > 2 {
            fds.push(fd);
        }
    }

    for fd in fds {
        match close(fd) {
            Ok(()) | Err(Errno::EBADF) => {}
            Err(source) => return Err(HardenError::CloseFd { fd, source }),
        }
    }

    Ok(())
}

/// Replace this process with the official jailer.
pub fn exec_jailer(args: HardenArgs) -> Result<(), HardenError> {
    let err = Command::new(&args.jailer_bin)
        .args(args.jailer_args)
        .env_clear()
        .exec();
    Err(HardenError::Exec {
        path: args.jailer_bin,
        source: err,
    })
}

/// Parse, harden, and exec.
pub fn run<I, S>(args: I) -> Result<(), HardenError>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let args = parse_args(args)?;
    apply_process_hardening()?;
    exec_jailer(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_requires_separator() {
        let err = parse_args([
            "--jailer-bin",
            "/bin/echo",
            "--uid",
            "3000",
            "--gid",
            "3000",
        ])
        .unwrap_err();
        assert!(matches!(err, HardenError::MissingSeparator));
    }

    #[test]
    fn parse_requires_jailer_args_after_separator() {
        let err = parse_args([
            "--jailer-bin",
            "/bin/echo",
            "--uid",
            "3000",
            "--gid",
            "3000",
            "--",
        ])
        .unwrap_err();
        assert!(matches!(err, HardenError::MissingJailerArgs));
    }

    #[test]
    fn parse_captures_official_jailer_and_forwarded_args() {
        let parsed = parse_args([
            "--jailer-bin",
            "/opt/firecracker/bin/jailer",
            "--uid",
            "3000",
            "--gid",
            "3001",
            "--",
            "--id",
            "vm-1",
        ])
        .unwrap();

        assert_eq!(
            parsed.jailer_bin,
            PathBuf::from("/opt/firecracker/bin/jailer")
        );
        assert_eq!(parsed.uid, 3000);
        assert_eq!(parsed.gid, 3001);
        assert_eq!(parsed.jailer_args, vec!["--id", "vm-1"]);
    }

    #[test]
    fn parse_rejects_bad_uid() {
        let err = parse_args([
            "--jailer-bin",
            "/bin/echo",
            "--uid",
            "not-a-uid",
            "--gid",
            "3000",
            "--",
            "--id",
            "vm-1",
        ])
        .unwrap_err();

        assert!(matches!(
            err,
            HardenError::InvalidValue { field: "--uid", .. }
        ));
    }
}
