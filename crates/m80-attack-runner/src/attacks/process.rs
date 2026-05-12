//! Process and PID-isolation attack attempts.

use std::fs;

use nix::sys::signal;
use nix::unistd::Pid;

use crate::{blocked, config, AttackBlocked, AttackResult};

pub(crate) fn observe_host_pid_status() -> AttackResult {
    let pid = host_pid()?;
    match fs::read_to_string(format!("/proc/{pid}/status")) {
        Ok(status) if status.contains("Name:") => Ok(()),
        Ok(_) => Err(AttackBlocked::new(format!(
            "proc status for pid {pid} was empty"
        ))),
        Err(err) => Err(blocked(format!("read /proc/{pid}/status"), err)),
    }
}

pub(crate) fn signal_host_pid_probe() -> AttackResult {
    let pid = Pid::from_raw(host_pid()?);
    signal::kill(pid, None).map_err(|err| blocked(format!("kill -0 {}", pid.as_raw()), err))
}

pub(crate) fn read_host_pid_cmdline() -> AttackResult {
    let pid = host_pid()?;
    match fs::read(format!("/proc/{pid}/cmdline")) {
        Ok(bytes) if !bytes.is_empty() => Ok(()),
        Ok(_) => Err(AttackBlocked::new(format!(
            "cmdline for pid {pid} was empty"
        ))),
        Err(err) => Err(blocked(format!("read /proc/{pid}/cmdline"), err)),
    }
}

pub(crate) fn enumerate_host_processes() -> AttackResult {
    let visible = fs::read_dir("/proc")
        .map_err(|err| blocked("read /proc", err))?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .chars()
                .all(|ch| ch.is_ascii_digit())
        })
        .take(64)
        .count();
    if visible > 32 {
        Ok(())
    } else {
        Err(AttackBlocked::new(format!(
            "only {visible} process directories visible"
        )))
    }
}

pub(crate) fn read_host_proc_mountinfo() -> AttackResult {
    let pid = host_pid()?;
    match fs::read_to_string(format!("/proc/{pid}/mountinfo")) {
        Ok(mountinfo) if mountinfo.contains(" / ") => Ok(()),
        Ok(_) => Err(AttackBlocked::new(format!(
            "mountinfo for pid {pid} was empty"
        ))),
        Err(err) => Err(blocked(format!("read /proc/{pid}/mountinfo"), err)),
    }
}

fn host_pid() -> Result<i32, AttackBlocked> {
    let raw = config::value("host_pid", "M80_ATTACK_HOST_PID", "1");
    parse_host_pid(&raw)
}

fn parse_host_pid(raw: &str) -> Result<i32, AttackBlocked> {
    raw.parse::<i32>()
        .map_err(|err| AttackBlocked::new(format!("invalid host_pid {raw}: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_host_pid_accepts_numeric_pid() {
        assert_eq!(parse_host_pid("4242").unwrap(), 4242);
    }

    #[test]
    fn parse_host_pid_rejects_invalid_pid() {
        let err = parse_host_pid("not-a-pid").unwrap_err();
        assert!(err.reason().contains("invalid host_pid"), "{err}");
    }
}
