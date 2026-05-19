//! Filesystem escape attempts.

use std::fs::{self, OpenOptions};
use std::io::Write as _;

use super::read_forbidden;
use crate::{blocked, host_sentinel, lower_sentinel, AttackResult};

pub(crate) fn chroot_escape_via_dotdot() -> AttackResult {
    read_forbidden(
        "../../../../../../etc/shadow",
        "dotdot reached host /etc/shadow",
    )
}

pub(crate) fn chroot_escape_via_openat_style_path() -> AttackResult {
    read_forbidden(
        "./../../../../../../root/.ssh/authorized_keys",
        "openat-style path escaped",
    )
}

pub(crate) fn chroot_escape_via_proc_self_root() -> AttackResult {
    let path = format!("/proc/self/root{}", host_sentinel());
    read_forbidden(&path, "proc self root exposed host sentinel")
}

pub(crate) fn read_host_sentinel() -> AttackResult {
    read_forbidden(&host_sentinel(), "host sentinel readable")
}

pub(crate) fn write_host_sentinel() -> AttackResult {
    let path = host_sentinel();
    match OpenOptions::new().append(true).open(&path) {
        Ok(mut file) => {
            file.write_all(b"m80 attack runner wrote host sentinel\n")
                .map_err(|err| blocked(format!("write {path}"), err))?;
            Ok(())
        }
        Err(err) => Err(blocked(format!("open {path} for append"), err)),
    }
}

pub(crate) fn write_to_lower_layer() -> AttackResult {
    let path = format!("{}/m80-attack-runner-write", lower_sentinel());
    match fs::write(&path, b"blocked") {
        Ok(()) => Ok(()),
        Err(err) => Err(blocked(format!("write {path}"), err)),
    }
}
