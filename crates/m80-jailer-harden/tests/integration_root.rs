//! Root-only integration tests for inherited process hardening.
//!
//! Run manually:
//!
//! ```sh
//! sudo cargo test -p m80-jailer-harden --test integration_root -- --ignored
//! ```

use std::os::fd::AsRawFd;
use std::process::Command;

use nix::fcntl::{fcntl, FcntlArg, FdFlag};

#[test]
#[ignore = "requires root or CAP_SETGID/CAP_SETPCAP"]
fn wrapper_applies_inherited_hardening_before_exec() {
    let wrapper = env!("CARGO_BIN_EXE_m80-jailer-harden");
    let inherited_file = tempfile::NamedTempFile::new().expect("inherited fd file");
    let inherited_fd = inherited_file.as_file().as_raw_fd();
    let inherited_path = inherited_file.path().to_string_lossy();
    let current_flags =
        FdFlag::from_bits_truncate(fcntl(inherited_fd, FcntlArg::F_GETFD).expect("get fd flags"));
    fcntl(
        inherited_fd,
        FcntlArg::F_SETFD(current_flags - FdFlag::FD_CLOEXEC),
    )
    .expect("clear cloexec");

    let output = Command::new(wrapper)
        .env("M80_SECRET_SHOULD_NOT_LEAK", "secret")
        .args([
            "--jailer-bin",
            "/bin/sh",
            "--uid",
            "3000",
            "--gid",
            "3000",
            "--",
            "-c",
            &format!(
                r#"if [ -n "$M80_SECRET_SHOULD_NOT_LEAK" ]; then echo env-leaked; exit 44; fi
if target=$(readlink /proc/self/fd/{inherited_fd} 2>/dev/null) && [ "$target" = "{inherited_path}" ]; then echo fd-leaked; exit 45; fi
printf 'UMASK=%s\n' "$(umask)"
grep -E '^(Groups|NoNewPrivs|CapInh|CapAmb|SigBlk):' /proc/self/status
"#,
                inherited_fd = inherited_fd,
                inherited_path = inherited_path
            ),
        ])
        .output()
        .expect("run wrapper");

    assert!(
        output.status.success(),
        "status={} stdout=\n{}\nstderr=\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("UMASK=0077"), "{stdout}");
    assert!(stdout.contains("NoNewPrivs:\t1"), "{stdout}");
    assert!(stdout.contains("CapInh:\t0000000000000000"), "{stdout}");
    assert!(stdout.contains("CapAmb:\t0000000000000000"), "{stdout}");
    assert!(stdout.contains("SigBlk:\t0000000000000000"), "{stdout}");

    let groups = stdout
        .lines()
        .find(|line| line.starts_with("Groups:"))
        .unwrap_or_else(|| panic!("missing Groups line:\n{stdout}"));
    assert_eq!(groups.trim(), "Groups:", "{stdout}");
}
