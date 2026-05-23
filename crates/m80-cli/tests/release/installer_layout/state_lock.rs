use std::fs;

use super::fixture::write_release_bundle;
use super::{
    clear_install_env, current_proc_start_ticks, m80, path_with_install_bin_first, run_install,
    seed_previous_active_install, write_install_lock, write_raw_install_lock, HostPrereqFixture,
};

#[test]
fn install_state_lock_blocks_second_writer_before_staging_or_activation() {
    let bundle = write_release_bundle(None);
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let previous = seed_previous_active_install(&install_root);
    write_install_lock(
        &install_root,
        std::process::id(),
        Some("v-lock-owner"),
        current_proc_start_ticks(),
    );

    let output = run_install(&bundle, &install_root, None, &[], &[]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("install.lock"), "{stderr}");
    assert!(
        stderr.contains(&format!("owner_pid={}", std::process::id())),
        "{stderr}"
    );
    assert!(stderr.contains("--repair-stale-install-lock"), "{stderr}");
    assert!(
        !install_root.join(".staging").exists(),
        "lock contention must fail before staging"
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        previous
    );
}

#[test]
fn stale_install_state_lock_requires_explicit_repair_flag() {
    let bundle = write_release_bundle(None);
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let previous = seed_previous_active_install(&install_root);
    let lock_path = write_install_lock(&install_root, 999_999_999, Some("v-stale"), 0);

    let output = run_install(&bundle, &install_root, None, &[], &[]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("install.lock"), "{stderr}");
    assert!(stderr.contains("owner_pid=999999999"), "{stderr}");
    assert!(stderr.contains("--repair-stale-install-lock"), "{stderr}");
    assert!(
        lock_path.exists(),
        "stale lock must remain without repair flag"
    );
    assert!(
        !install_root.join(".staging").exists(),
        "stale lock refusal must fail before staging"
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        previous
    );
}

#[test]
fn repair_stale_install_state_lock_rejects_unreadable_lock_record() {
    let bundle = write_release_bundle(None);
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let previous = seed_previous_active_install(&install_root);
    let lock_path = write_raw_install_lock(&install_root, b"{");

    let mut command = m80();
    command.args([
        "install",
        "--bundle-url",
        &format!("file://{}", bundle.tarball.display()),
        "--install-root",
        install_root.to_str().unwrap(),
        "--repair-stale-install-lock",
    ]);
    clear_install_env(&mut command);
    let output = command.output().unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("install.lock"), "{stderr}");
    assert!(stderr.contains("owner=<unreadable>"), "{stderr}");
    assert!(stderr.contains("--repair-stale-install-lock"), "{stderr}");
    assert!(
        lock_path.exists(),
        "unreadable lock records must fail closed"
    );
    assert!(
        !install_root.join(".staging").exists(),
        "unreadable lock refusal must fail before staging"
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        previous
    );
}

#[test]
fn repair_stale_install_state_lock_ignores_reused_pid() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let lock_path = write_install_lock(&install_root, std::process::id(), Some("v-reused-pid"), 0);

    let mut command = m80();
    command.args([
        "install",
        "--bundle-url",
        &format!("file://{}", bundle.tarball.display()),
        "--install-root",
        install_root.to_str().unwrap(),
        "--repair-stale-install-lock",
    ]);
    clear_install_env(&mut command);
    command.env("PATH", path_with_install_bin_first(&install_root));
    host.apply(&mut command);
    let output = command.output().unwrap();

    assert!(
        output.status.success(),
        "reused-pid stale lock repair failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !lock_path.exists(),
        "successful install must release repaired lock"
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        install_root.join("versions").join(&bundle.release_tag)
    );
}

#[test]
fn repair_stale_install_state_lock_then_installs_normally() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let lock_path = write_install_lock(&install_root, 999_999_999, Some("v-stale"), 0);

    let mut command = m80();
    command.args([
        "install",
        "--bundle-url",
        &format!("file://{}", bundle.tarball.display()),
        "--install-root",
        install_root.to_str().unwrap(),
        "--repair-stale-install-lock",
    ]);
    clear_install_env(&mut command);
    command.env("PATH", path_with_install_bin_first(&install_root));
    host.apply(&mut command);
    let output = command.output().unwrap();

    assert!(
        output.status.success(),
        "stale lock repair install failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert!(
        !lock_path.exists(),
        "successful install must release install-state lock"
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        install_root.join("versions").join(&bundle.release_tag)
    );
}
