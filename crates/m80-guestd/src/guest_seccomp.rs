//! Fixed seccomp profiles for long-lived guestd and workload children.

use std::collections::BTreeMap;
use std::convert::TryInto;
use std::process::Command;

use anyhow::Context as _;
use nix::sched::CloneFlags;
use seccompiler::{BpfProgram, SeccompAction, SeccompFilter, SeccompRule};

/// Hidden CLI argument used by tests and real-guest smoke probes.
pub(crate) const SECCOMP_PROBE_ARG: &str = "--m80-seccomp-probe";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SeccompProbe {
    DaemonMode,
    WorkloadMode,
    WorkloadDeniedUnshare,
    WorkloadOrdinaryExec,
}

/// Parse the hidden seccomp probe mode.
pub(crate) fn parse_seccomp_probe_args(
    mut args: impl Iterator<Item = String>,
) -> anyhow::Result<SeccompProbe> {
    let Some(profile) = args.next() else {
        anyhow::bail!("{SECCOMP_PROBE_ARG} requires a profile");
    };
    let Some(action) = args.next() else {
        anyhow::bail!("{SECCOMP_PROBE_ARG} requires an action");
    };
    if args.next().is_some() {
        anyhow::bail!("{SECCOMP_PROBE_ARG} accepts exactly two values");
    }

    match (profile.as_str(), action.as_str()) {
        ("daemon", "mode") => Ok(SeccompProbe::DaemonMode),
        ("workload", "mode") => Ok(SeccompProbe::WorkloadMode),
        ("workload", "deny-unshare") => Ok(SeccompProbe::WorkloadDeniedUnshare),
        ("workload", "ordinary-exec") => Ok(SeccompProbe::WorkloadOrdinaryExec),
        _ => anyhow::bail!("unknown seccomp probe {profile} {action}"),
    }
}

/// Install the long-lived guestd daemon profile.
pub(crate) fn install_daemon_filter() -> anyhow::Result<()> {
    install_profile(daemon_denied_syscalls()).context("apply guestd daemon seccomp filter")
}

/// Install the fixed workload profile after UID/GID/capability drop.
pub(crate) fn install_workload_filter() -> anyhow::Result<()> {
    install_profile(workload_denied_syscalls()).context("apply guest workload seccomp filter")
}

/// Run a hidden seccomp probe and print one line of machine-readable evidence.
pub(crate) fn run_seccomp_probe(probe: SeccompProbe) -> anyhow::Result<()> {
    match probe {
        SeccompProbe::DaemonMode => {
            install_daemon_filter()?;
            println!("seccomp={}", current_thread_seccomp_mode()?);
        }
        SeccompProbe::WorkloadMode => {
            install_workload_filter()?;
            println!("seccomp={}", current_thread_seccomp_mode()?);
        }
        SeccompProbe::WorkloadDeniedUnshare => {
            install_workload_filter()?;
            match nix::sched::unshare(CloneFlags::CLONE_FS) {
                Err(nix::errno::Errno::EPERM) => println!("unshare=blocked"),
                Err(err) => anyhow::bail!("unshare returned unexpected error: {err}"),
                Ok(()) => anyhow::bail!("unshare unexpectedly succeeded under workload profile"),
            }
        }
        SeccompProbe::WorkloadOrdinaryExec => {
            install_workload_filter()?;
            let status = Command::new("/bin/true")
                .status()
                .context("spawn /bin/true under workload seccomp profile")?;
            if !status.success() {
                anyhow::bail!("/bin/true exited with status {status}");
            }
            println!("ordinary_exec=ok");
        }
    }
    Ok(())
}

fn install_profile(denied_syscalls: &[i64]) -> anyhow::Result<()> {
    let filter = build_errno_filter(denied_syscalls)?;
    seccompiler::apply_filter(&filter).map_err(anyhow::Error::from)
}

fn build_errno_filter(denied_syscalls: &[i64]) -> anyhow::Result<BpfProgram> {
    let rules: BTreeMap<i64, Vec<SeccompRule>> = denied_syscalls
        .iter()
        .copied()
        .map(|syscall| (syscall, Vec::new()))
        .collect();
    SeccompFilter::new(
        rules,
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM as u32),
        std::env::consts::ARCH.try_into()?,
    )?
    .try_into()
    .map_err(anyhow::Error::from)
}

fn daemon_denied_syscalls() -> &'static [i64] {
    &[
        libc::SYS_add_key,
        libc::SYS_bpf,
        libc::SYS_delete_module,
        libc::SYS_finit_module,
        libc::SYS_init_module,
        libc::SYS_io_uring_register,
        libc::SYS_io_uring_setup,
        libc::SYS_kcmp,
        libc::SYS_kexec_file_load,
        libc::SYS_kexec_load,
        libc::SYS_keyctl,
        libc::SYS_lookup_dcookie,
        libc::SYS_open_by_handle_at,
        libc::SYS_perf_event_open,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_ptrace,
        libc::SYS_request_key,
        libc::SYS_userfaultfd,
    ]
}

fn workload_denied_syscalls() -> &'static [i64] {
    &[
        libc::SYS_add_key,
        libc::SYS_bpf,
        libc::SYS_delete_module,
        libc::SYS_finit_module,
        libc::SYS_init_module,
        libc::SYS_io_uring_register,
        libc::SYS_io_uring_setup,
        libc::SYS_kcmp,
        libc::SYS_kexec_file_load,
        libc::SYS_kexec_load,
        libc::SYS_keyctl,
        libc::SYS_lookup_dcookie,
        libc::SYS_mount,
        libc::SYS_open_by_handle_at,
        libc::SYS_perf_event_open,
        libc::SYS_pivot_root,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_ptrace,
        libc::SYS_reboot,
        libc::SYS_request_key,
        libc::SYS_setns,
        libc::SYS_swapoff,
        libc::SYS_swapon,
        libc::SYS_umount2,
        libc::SYS_unshare,
        libc::SYS_userfaultfd,
    ]
}

fn current_thread_seccomp_mode() -> anyhow::Result<u32> {
    let status = std::fs::read_to_string("/proc/thread-self/status")
        .context("read /proc/thread-self/status")?;
    seccomp_mode_from_status(&status).context("Seccomp field missing from thread status")
}

fn seccomp_mode_from_status(status: &str) -> Option<u32> {
    status.lines().find_map(|line| {
        let value = line.strip_prefix("Seccomp:")?;
        value.trim().parse().ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_probe_requires_profile_and_action() {
        let err = parse_seccomp_probe_args(std::iter::empty()).unwrap_err();
        assert!(err.to_string().contains("requires a profile"));
    }

    #[test]
    fn parse_probe_rejects_extra_values() {
        let err =
            parse_seccomp_probe_args(["daemon", "mode", "extra"].into_iter().map(str::to_owned))
                .unwrap_err();
        assert!(err.to_string().contains("exactly two values"));
    }

    #[test]
    fn parse_known_probes() {
        assert_eq!(
            parse_seccomp_probe_args(["daemon", "mode"].into_iter().map(str::to_owned)).unwrap(),
            SeccompProbe::DaemonMode
        );
        assert_eq!(
            parse_seccomp_probe_args(["workload", "deny-unshare"].into_iter().map(str::to_owned))
                .unwrap(),
            SeccompProbe::WorkloadDeniedUnshare
        );
    }

    #[test]
    fn daemon_profile_does_not_block_workload_filter_install_syscalls() {
        assert!(!daemon_denied_syscalls().contains(&libc::SYS_prctl));
        assert!(!daemon_denied_syscalls().contains(&libc::SYS_seccomp));
    }

    #[test]
    fn workload_profile_denies_namespace_and_mount_primitives() {
        assert!(workload_denied_syscalls().contains(&libc::SYS_unshare));
        assert!(workload_denied_syscalls().contains(&libc::SYS_mount));
        assert!(workload_denied_syscalls().contains(&libc::SYS_setns));
    }

    #[test]
    fn status_parser_reads_seccomp_mode() {
        let status = "Name:\tm80-guestd\nSeccomp:\t2\n";
        assert_eq!(seccomp_mode_from_status(status), Some(2));
    }
}
