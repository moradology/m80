//! Process-hardening wrapper used before execing Firecracker's official jailer.

use std::ffi::{OsStr, OsString};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use caps::{CapSet, Capability, CapsHashSet};
use nix::sched::{unshare, CloneFlags};
use nix::sys::prctl;
use nix::sys::resource::{setrlimit, Resource};
use nix::sys::signal::{SigSet, SigmaskHow, Signal};
use nix::sys::stat::{umask, Mode};
use nix::unistd::setgroups;

const OFFICIAL_JAILER_CAPABILITIES: &[Capability] = &[
    Capability::CAP_CHOWN,
    Capability::CAP_DAC_OVERRIDE,
    Capability::CAP_SYS_CHROOT,
    Capability::CAP_MKNOD,
    Capability::CAP_SETUID,
    Capability::CAP_SETGID,
    Capability::CAP_SYS_ADMIN,
];

/// Parsed command-line input for `m80-jailer-harden`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardenArgs {
    /// Official Firecracker jailer binary to exec after hardening.
    pub(crate) jailer_bin: PathBuf,
    /// Arguments forwarded to the official jailer.
    pub(crate) jailer_args: Vec<OsString>,
    /// Resource limits to apply before execing the official jailer.
    pub(crate) resource_limits: Vec<ResourceLimit>,
    /// Enter a private cgroup namespace before execing the official jailer.
    pub(crate) new_cgroup_ns: bool,
    /// Enter a private network namespace before execing the official jailer.
    pub(crate) new_net_ns: bool,
}

impl HardenArgs {
    /// Resource limits requested by the parsed wrapper arguments.
    #[must_use]
    pub fn resource_limits(&self) -> &[ResourceLimit] {
        &self.resource_limits
    }

    /// Whether the wrapper should enter a new cgroup namespace before exec.
    #[must_use]
    pub fn new_cgroup_ns(&self) -> bool {
        self.new_cgroup_ns
    }

    /// Whether the wrapper should enter a new network namespace before exec.
    #[must_use]
    pub fn new_net_ns(&self) -> bool {
        self.new_net_ns
    }
}

/// One process resource limit applied by `m80-jailer-harden`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceLimit {
    /// Limit kind.
    pub(crate) kind: ResourceLimitKind,
    /// Soft and hard limit value.
    pub(crate) value: u64,
}

/// Supported process resource limit kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceLimitKind {
    /// `RLIMIT_NOFILE`.
    NoFile,
    /// `RLIMIT_FSIZE`.
    FSize,
    /// `RLIMIT_NPROC`.
    NProc,
    /// `RLIMIT_MEMLOCK`.
    MemLock,
    /// `RLIMIT_AS`.
    AddressSpace,
    /// `RLIMIT_CORE`.
    Core,
    /// `RLIMIT_STACK`.
    Stack,
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
        /// Argument or field name.
        field: &'static str,
        /// Rejected value.
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
    #[error("prune {set} capabilities: {source}")]
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
    /// `unshare(CLONE_NEWCGROUP)` failed.
    #[error("unshare cgroup namespace: {0}")]
    CgroupNamespace(#[source] nix::Error),
    /// `unshare(CLONE_NEWNET)` failed.
    #[error("unshare network namespace: {0}")]
    NetworkNamespace(#[source] nix::Error),
    /// `setrlimit` failed.
    #[error("setrlimit {kind}: {source}")]
    SetResourceLimit {
        /// Limit kind.
        kind: &'static str,
        /// Source error.
        source: nix::Error,
    },
    /// Signal mask reset failed.
    #[error("reset signal mask: {0}")]
    SignalMask(#[source] nix::Error),
    /// Inherited file descriptors could not be closed.
    #[error("close inherited file descriptors: {0}")]
    CloseRange(#[source] std::io::Error),
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
    let mut resource_limits = Vec::new();
    let mut new_cgroup_ns = false;
    let mut new_net_ns = false;

    while let Some(arg) = iter.next() {
        if arg == OsStr::new("--") {
            let jailer_args: Vec<_> = iter.collect();
            if jailer_args.is_empty() {
                return Err(HardenError::MissingJailerArgs);
            }
            let _uid = uid.ok_or(HardenError::MissingArgument("--uid"))?;
            let _gid = gid.ok_or(HardenError::MissingArgument("--gid"))?;
            return Ok(HardenArgs {
                jailer_bin: jailer_bin.ok_or(HardenError::MissingArgument("--jailer-bin"))?,
                jailer_args,
                resource_limits,
                new_cgroup_ns,
                new_net_ns,
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
            "--rlimit" => {
                let value = iter
                    .next()
                    .ok_or(HardenError::MissingArgument("--rlimit"))?;
                resource_limits.push(parse_resource_limit(value)?);
            }
            "--new-cgroup-ns" => {
                new_cgroup_ns = true;
            }
            "--new-net-ns" => {
                new_net_ns = true;
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

fn parse_resource_limit(value: OsString) -> Result<ResourceLimit, HardenError> {
    let value = value.to_string_lossy().into_owned();
    let (name, raw) = value
        .split_once('=')
        .ok_or_else(|| HardenError::InvalidValue {
            field: "--rlimit",
            value: value.clone(),
        })?;
    let limit = raw.parse().map_err(|_| HardenError::InvalidValue {
        field: "--rlimit",
        value: value.clone(),
    })?;
    let kind = match name {
        "no-file" => ResourceLimitKind::NoFile,
        "fsize" => ResourceLimitKind::FSize,
        "nproc" => ResourceLimitKind::NProc,
        "memlock" => ResourceLimitKind::MemLock,
        "as" => ResourceLimitKind::AddressSpace,
        "core" => ResourceLimitKind::Core,
        "stack" => ResourceLimitKind::Stack,
        _ => {
            return Err(HardenError::InvalidValue {
                field: "--rlimit",
                value,
            })
        }
    };
    Ok(ResourceLimit { kind, value: limit })
}

/// Apply inherited one-way process hardening before execing the official jailer.
pub fn apply_process_hardening(
    resource_limits: &[ResourceLimit],
    new_cgroup_ns: bool,
    new_net_ns: bool,
) -> Result<(), HardenError> {
    if new_cgroup_ns {
        unshare(CloneFlags::CLONE_NEWCGROUP).map_err(HardenError::CgroupNamespace)?;
    }
    if new_net_ns {
        unshare(CloneFlags::CLONE_NEWNET).map_err(HardenError::NetworkNamespace)?;
    }
    apply_resource_limits(resource_limits)?;
    setgroups(&[]).map_err(HardenError::SetGroups)?;
    caps::clear(None, CapSet::Inheritable).map_err(|source| HardenError::ClearCaps {
        set: "inheritable",
        source,
    })?;
    caps::clear(None, CapSet::Ambient).map_err(|source| HardenError::ClearCaps {
        set: "ambient",
        source,
    })?;
    prune_bounding_capabilities()?;
    retain_official_jailer_capabilities(CapSet::Effective, "effective")?;
    retain_official_jailer_capabilities(CapSet::Permitted, "permitted")?;
    prctl::set_no_new_privs().map_err(HardenError::NoNewPrivs)?;
    prctl::set_pdeathsig(Signal::SIGKILL).map_err(HardenError::ParentDeathSignal)?;
    umask(Mode::from_bits_truncate(0o077));
    let empty = SigSet::empty();
    nix::sys::signal::pthread_sigmask(SigmaskHow::SIG_SETMASK, Some(&empty), None)
        .map_err(HardenError::SignalMask)?;
    close_inherited_fds()?;
    Ok(())
}

fn official_jailer_capability_set() -> CapsHashSet {
    OFFICIAL_JAILER_CAPABILITIES.iter().copied().collect()
}

fn prune_bounding_capabilities() -> Result<(), HardenError> {
    let allowed = official_jailer_capability_set();
    let current = caps::read(None, CapSet::Bounding).map_err(|source| HardenError::ClearCaps {
        set: "bounding",
        source,
    })?;
    for cap in current {
        if !allowed.contains(&cap) {
            caps::drop(None, CapSet::Bounding, cap).map_err(|source| HardenError::ClearCaps {
                set: "bounding",
                source,
            })?;
        }
    }
    Ok(())
}

fn retain_official_jailer_capabilities(
    cap_set: CapSet,
    set_name: &'static str,
) -> Result<(), HardenError> {
    let allowed = official_jailer_capability_set();
    let mut current = caps::read(None, cap_set).map_err(|source| HardenError::ClearCaps {
        set: set_name,
        source,
    })?;
    current.retain(|cap| allowed.contains(cap));
    caps::set(None, cap_set, &current).map_err(|source| HardenError::ClearCaps {
        set: set_name,
        source,
    })
}

fn apply_resource_limits(resource_limits: &[ResourceLimit]) -> Result<(), HardenError> {
    for limit in resource_limits {
        let resource = match limit.kind {
            ResourceLimitKind::NoFile => Resource::RLIMIT_NOFILE,
            ResourceLimitKind::FSize => Resource::RLIMIT_FSIZE,
            ResourceLimitKind::NProc => Resource::RLIMIT_NPROC,
            ResourceLimitKind::MemLock => Resource::RLIMIT_MEMLOCK,
            ResourceLimitKind::AddressSpace => Resource::RLIMIT_AS,
            ResourceLimitKind::Core => Resource::RLIMIT_CORE,
            ResourceLimitKind::Stack => Resource::RLIMIT_STACK,
        };
        setrlimit(resource, limit.value, limit.value).map_err(|source| {
            HardenError::SetResourceLimit {
                kind: limit.kind.name(),
                source,
            }
        })?;
    }
    Ok(())
}

impl ResourceLimitKind {
    fn name(self) -> &'static str {
        match self {
            Self::NoFile => "no-file",
            Self::FSize => "fsize",
            Self::NProc => "nproc",
            Self::MemLock => "memlock",
            Self::AddressSpace => "as",
            Self::Core => "core",
            Self::Stack => "stack",
        }
    }
}

fn close_inherited_fds() -> Result<(), HardenError> {
    m80_close_range::close_from(3).map_err(HardenError::CloseRange)
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
            "--rlimit",
            "nproc=64",
            "--rlimit",
            "memlock=0",
            "--new-cgroup-ns",
            "--",
            "--uid",
            "3000",
            "--gid",
            "3001",
            "--id",
            "vm-1",
        ])
        .unwrap();

        assert_eq!(
            parsed.jailer_bin,
            PathBuf::from("/opt/firecracker/bin/jailer")
        );
        assert_eq!(
            parsed.jailer_args,
            vec!["--uid", "3000", "--gid", "3001", "--id", "vm-1"]
        );
        assert_eq!(
            parsed.resource_limits(),
            &[
                ResourceLimit {
                    kind: ResourceLimitKind::NProc,
                    value: 64,
                },
                ResourceLimit {
                    kind: ResourceLimitKind::MemLock,
                    value: 0,
                },
            ]
        );
        assert_eq!(
            parsed.resource_limits,
            vec![
                ResourceLimit {
                    kind: ResourceLimitKind::NProc,
                    value: 64,
                },
                ResourceLimit {
                    kind: ResourceLimitKind::MemLock,
                    value: 0,
                },
            ]
        );
        assert!(parsed.new_cgroup_ns());
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

    #[test]
    fn parse_requires_uid_before_separator() {
        let err = parse_args([
            "--jailer-bin",
            "/bin/echo",
            "--gid",
            "3000",
            "--",
            "--id",
            "vm",
        ])
        .unwrap_err();
        assert!(matches!(err, HardenError::MissingArgument("--uid")));
    }

    #[test]
    fn parse_requires_gid_before_separator() {
        let err = parse_args([
            "--jailer-bin",
            "/bin/echo",
            "--uid",
            "3000",
            "--",
            "--id",
            "vm",
        ])
        .unwrap_err();
        assert!(matches!(err, HardenError::MissingArgument("--gid")));
    }

    #[test]
    fn parse_rejects_unknown_resource_limit() {
        let err = parse_args([
            "--jailer-bin",
            "/bin/echo",
            "--uid",
            "3000",
            "--gid",
            "3000",
            "--rlimit",
            "unknown=7",
            "--",
            "--id",
            "vm-1",
        ])
        .unwrap_err();

        assert!(matches!(
            err,
            HardenError::InvalidValue {
                field: "--rlimit",
                ..
            }
        ));
    }

    #[test]
    fn official_jailer_capability_allowlist_is_pinned() {
        let allowed = official_jailer_capability_set();

        assert_eq!(allowed.len(), 7);
        assert!(allowed.contains(&Capability::CAP_CHOWN));
        assert!(allowed.contains(&Capability::CAP_DAC_OVERRIDE));
        assert!(allowed.contains(&Capability::CAP_SYS_CHROOT));
        assert!(allowed.contains(&Capability::CAP_MKNOD));
        assert!(allowed.contains(&Capability::CAP_SETUID));
        assert!(allowed.contains(&Capability::CAP_SETGID));
        assert!(allowed.contains(&Capability::CAP_SYS_ADMIN));
    }

    #[test]
    fn pre_jailer_dangerous_caps_are_not_allowed() {
        let allowed = official_jailer_capability_set();

        for cap in [
            Capability::CAP_NET_ADMIN,
            Capability::CAP_KILL,
            Capability::CAP_FOWNER,
            Capability::CAP_SYS_PTRACE,
            Capability::CAP_SYS_MODULE,
            Capability::CAP_SYS_RAWIO,
            Capability::CAP_SETPCAP,
        ] {
            assert!(!allowed.contains(&cap), "{cap} must be pruned");
        }
    }
}
