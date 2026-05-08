//! Privilege escalation attack attempts.

use std::fs;

use nix::mount::{mount, MsFlags};
use nix::sched::{unshare, CloneFlags};
use nix::unistd::{setgid, setuid, Gid, Uid};

use crate::{blocked, AttackBlocked, AttackResult};

pub(crate) fn become_uid_zero() -> AttackResult {
    setuid(Uid::from_raw(0)).map_err(|err| blocked("setuid(0)", err))?;
    if Uid::effective().is_root() {
        Ok(())
    } else {
        Err(AttackBlocked::new(
            "setuid(0) did not yield effective uid 0",
        ))
    }
}

pub(crate) fn become_gid_zero() -> AttackResult {
    setgid(Gid::from_raw(0)).map_err(|err| blocked("setgid(0)", err))
}

pub(crate) fn retain_effective_capabilities() -> AttackResult {
    let status = fs::read_to_string("/proc/self/status")
        .map_err(|err| blocked("read /proc/self/status", err))?;
    let cap_eff = status
        .lines()
        .find_map(|line| line.strip_prefix("CapEff:\t"))
        .ok_or_else(|| AttackBlocked::new("CapEff missing from /proc/self/status"))?;
    if cap_eff.trim() == "0000000000000000" {
        Err(AttackBlocked::new("CapEff is zero"))
    } else {
        Ok(())
    }
}

pub(crate) fn unshare_mount_namespace() -> AttackResult {
    unshare(CloneFlags::CLONE_NEWNS).map_err(|err| blocked("unshare(CLONE_NEWNS)", err))
}

pub(crate) fn mount_tmpfs() -> AttackResult {
    mount(
        Some("tmpfs"),
        "/mnt",
        Some("tmpfs"),
        MsFlags::empty(),
        Some("size=64k"),
    )
    .map_err(|err| blocked("mount tmpfs /mnt", err))
}

pub(crate) fn change_hostname() -> AttackResult {
    std::fs::write("/proc/sys/kernel/hostname", b"m80-attack-runner\n")
        .map_err(|err| blocked("write /proc/sys/kernel/hostname", err))
}
