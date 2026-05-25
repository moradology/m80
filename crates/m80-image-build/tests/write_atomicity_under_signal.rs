//! Ignored real-host regression for image-build SIGKILL atomicity.
//!
//! Run manually as root or with CAP_SYS_ADMIN:
//!
//! ```sh
//! sudo cargo test -p m80-image-build --test write_atomicity_under_signal -- --ignored
//! ```

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use assert_cmd::cargo::cargo_bin;
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;

#[test]
#[ignore = "requires-root requires-loop-device"]
fn image_build_write_atomicity_under_signal() {
    if !cfg!(debug_assertions) {
        eprintln!("release builds do not include the debug-only loop-mount sleep hook");
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let bin_dir = dir.path().join("bin");
    let out_dir = dir.path().join("out");
    fs::create_dir(&bin_dir).expect("create fake PATH dir");
    fs::create_dir(&out_dir).expect("create output dir");
    write_fake_curl(&bin_dir.join("curl"));

    let guestd = dir.path().join("fake-guestd");
    fs::write(&guestd, b"fake guestd").expect("write fake guestd");
    let config = dir.path().join("m80-image-build.toml");
    fs::write(
        &config,
        format!(
            r#"
[kernel]
version = "v1.15.1"
artifact_track = "v1.15"
arch = "x86_64"

[rootfs]
kind = "minimal"
size = "64MiB"

[guestd]
binary = "{}"

[output]
dir = "{}"
"#,
            guestd.display(),
            out_dir.display()
        ),
    )
    .expect("write config");

    let ready = dir.path().join("loop-mounted.txt");
    let child = Command::new(cargo_bin("m80-image-build"))
        .args(["run", "--config"])
        .arg(&config)
        .env("M80_TEST_SLEEP_AFTER_IMAGE_BUILD_LOOP_MOUNT", &ready)
        .env("PATH", prepend_path(&bin_dir))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn m80-image-build");

    let (child, mount_dir) = wait_for_loop_mount_marker(&ready, child);

    kill(Pid::from_raw(child.id() as i32), Signal::SIGKILL).expect("SIGKILL image-build");
    let output = child
        .wait_with_output()
        .expect("wait for killed image-build");
    assert_eq!(
        output.status.signal(),
        Some(9),
        "expected SIGKILL status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let rootfs = out_dir.join("output.ext4");
    let manifest = PathBuf::from(format!("{}.manifest.json", rootfs.display()));
    assert!(
        !manifest.exists(),
        "manifest must not be present after SIGKILL before successful build: {}",
        manifest.display()
    );
    assert!(
        !is_mounted(&mount_dir),
        "loop mount must not leak into host mount namespace: {}",
        mount_dir.display()
    );

    fs::remove_dir_all(&out_dir).expect("output dir must be cleanable after SIGKILL");
}

fn write_fake_curl(path: &Path) {
    fs::write(
        path,
        r#"#!/bin/sh
set -eu
dest=
while [ "$#" -gt 0 ]; do
    if [ "$1" = "-o" ]; then
        shift
        dest="$1"
    fi
    shift || true
done
if [ -z "$dest" ]; then
    echo "missing -o" >&2
    exit 64
fi
printf 'fake-kernel' > "$dest"
"#,
    )
    .expect("write fake curl");
    let mut perms = fs::metadata(path).expect("stat fake curl").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).expect("chmod fake curl");
}

fn prepend_path(dir: &Path) -> String {
    let prior = std::env::var_os("PATH").unwrap_or_default();
    format!("{}:{}", dir.display(), prior.to_string_lossy())
}

fn wait_for_loop_mount_marker(
    path: &Path,
    mut child: std::process::Child,
) -> (std::process::Child, PathBuf) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if path.exists() {
            let mount_dir = fs::read_to_string(path).expect("read loop mount marker");
            return (child, PathBuf::from(mount_dir.trim()));
        }
        if child.try_wait().expect("poll image-build child").is_some() {
            let output = child
                .wait_with_output()
                .expect("collect exited image-build");
            panic!(
                "image-build exited before loop-mount hook; status={:?}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(50));
    }

    let _ = kill(Pid::from_raw(child.id() as i32), Signal::SIGKILL);
    let output = child.wait_with_output().expect("wait after marker timeout");
    panic!(
        "image-build did not reach loop-mount hook before timeout; status={:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn is_mounted(path: &Path) -> bool {
    let mountinfo = fs::read_to_string("/proc/self/mountinfo").expect("read mountinfo");
    mountinfo
        .lines()
        .any(|line| line.split_whitespace().nth(4) == Some(path.to_string_lossy().as_ref()))
}
