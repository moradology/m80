//! Per-workload privilege boundary for guest exec children.

use std::os::unix::process::CommandExt;
use std::process::Command;

use anyhow::Context as _;
use caps::CapSet;
use nix::unistd::{Gid, Uid};

pub(crate) const EXEC_UID: u32 = 1000;
pub(crate) const EXEC_GID: u32 = 1000;
pub(crate) const EXEC_SHIM_ARG: &str = "--m80-exec-shim";
pub(crate) const EXEC_SHIM_PATH: &str = "/proc/self/exe";

/// Parsed hidden shim invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecShimRequest {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
}

pub(crate) fn root_exec_requires_shim(euid: Uid) -> bool {
    euid.is_root()
}

pub(crate) fn command_program_and_args(program: &str, args: &[String]) -> (String, Vec<String>) {
    if root_exec_requires_shim(nix::unistd::geteuid()) {
        (EXEC_SHIM_PATH.to_owned(), shim_args(program, args))
    } else {
        (program.to_owned(), args.to_vec())
    }
}

pub(crate) fn shim_args(program: &str, args: &[String]) -> Vec<String> {
    let mut shimmed = Vec::with_capacity(args.len() + 2);
    shimmed.push(EXEC_SHIM_ARG.to_owned());
    shimmed.push(program.to_owned());
    shimmed.extend(args.iter().cloned());
    shimmed
}

pub(crate) fn parse_exec_shim_args(mut args: impl Iterator<Item = String>) -> ExecShimRequest {
    let program = args
        .next()
        .expect("--m80-exec-shim parser called without target program");
    let args = args.collect();
    ExecShimRequest { program, args }
}

/// Drop guest-root privileges for one workload and exec the requested program.
///
/// This function runs inside the hidden shim process, not inside long-lived
/// guestd. Any failure exits that child and is reported as an exec failure.
pub(crate) fn run_exec_shim(req: ExecShimRequest) -> anyhow::Result<()> {
    apply_exec_privilege_drop().context("failed to apply guest exec privilege drop")?;
    crate::guest_seccomp::install_workload_filter()
        .context("failed to install guest exec seccomp profile")?;
    let err = Command::new(&req.program).args(&req.args).exec();
    Err(err).with_context(|| format!("failed to exec workload program {}", req.program))
}

fn apply_exec_privilege_drop() -> anyhow::Result<()> {
    nix::sys::prctl::set_no_new_privs().context("failed to set PR_SET_NO_NEW_PRIVS")?;
    clear_cap_set(CapSet::Ambient).context("failed to clear ambient capabilities")?;
    clear_cap_set(CapSet::Bounding).context("failed to clear bounding capabilities")?;

    let gid = Gid::from_raw(EXEC_GID);
    let uid = Uid::from_raw(EXEC_UID);
    nix::unistd::setgroups(&[gid]).context("failed to set exec supplemental groups")?;
    nix::unistd::setresgid(gid, gid, gid).context("failed to drop exec gid")?;
    nix::unistd::setresuid(uid, uid, uid).context("failed to drop exec uid")?;

    clear_cap_set(CapSet::Effective).context("failed to clear effective capabilities")?;
    clear_cap_set(CapSet::Permitted).context("failed to clear permitted capabilities")?;
    clear_cap_set(CapSet::Inheritable).context("failed to clear inheritable capabilities")?;
    Ok(())
}

fn clear_cap_set(set: CapSet) -> anyhow::Result<()> {
    caps::clear(None, set).map_err(anyhow::Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_euid_uses_exec_shim() {
        assert!(root_exec_requires_shim(Uid::from_raw(0)));
    }

    #[test]
    fn non_root_euid_uses_direct_exec() {
        assert!(!root_exec_requires_shim(Uid::from_raw(1000)));
    }

    #[test]
    fn shim_args_preserve_program_and_args() {
        let args = vec!["-c".to_owned(), "printf %s \"$1\"".to_owned()];
        assert_eq!(
            shim_args("/bin/sh", &args),
            vec![
                EXEC_SHIM_ARG.to_owned(),
                "/bin/sh".to_owned(),
                "-c".to_owned(),
                "printf %s \"$1\"".to_owned()
            ]
        );
    }

    #[test]
    fn parse_exec_shim_args_collects_target_args_verbatim() {
        let parsed = parse_exec_shim_args(
            ["/bin/sh", "-c", "printf ok", "--port", "9001"]
                .into_iter()
                .map(str::to_owned),
        );
        assert_eq!(parsed.program, "/bin/sh");
        assert_eq!(parsed.args, ["-c", "printf ok", "--port", "9001"]);
    }
}
