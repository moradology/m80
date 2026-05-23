use std::fs;

use super::fixture::write_release_bundle;
use super::{
    run_install, run_install_with_extra_args, seed_previous_active_install, HostPrereqFixture,
};

#[test]
fn install_bundle_layout_accepts_matching_existing_selector_files() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");

    let first = run_install(&bundle, &install_root, Some(&host), &[], &[]);
    assert!(
        first.status.success(),
        "seed install failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );
    fs::remove_file(install_root.join("active")).unwrap();
    fs::remove_file(install_root.join("bin/m80")).unwrap();
    fs::remove_dir_all(install_root.join("versions").join(&bundle.release_tag)).unwrap();

    let second = run_install(&bundle, &install_root, Some(&host), &[], &[]);

    assert!(
        second.status.success(),
        "matching selector install failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );
}

#[test]
fn install_bundle_layout_rejects_conflicting_operator_config() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    fs::create_dir_all(&install_root).unwrap();
    let config_path = install_root.join("config.toml");
    let operator_config = "default_profile = 'operator'\nrun_root = '/operator/run'\n";
    fs::write(&config_path, operator_config).unwrap();

    let output = run_install(&bundle, &install_root, Some(&host), &[], &[]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("existing m80 config would be overwritten"),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!("old_path={}", config_path.display())),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!("proposed_path={}", config_path.display())),
        "{stderr}"
    );
    assert!(stderr.contains("--adopt-existing-config"), "{stderr}");
    assert!(
        stderr.contains(&format!(
            "cp -a '{}' '{}.backup'",
            config_path.display(),
            config_path.display()
        )),
        "{stderr}"
    );
    assert_eq!(fs::read_to_string(&config_path).unwrap(), operator_config);
    assert!(!install_root.join("active").exists());
}

#[test]
fn install_bundle_layout_rejects_conflicting_operator_profile() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let profile_path = install_root.join("profiles/default.toml");
    fs::create_dir_all(profile_path.parent().unwrap()).unwrap();
    let operator_profile = "description = 'operator profile'\n";
    fs::write(&profile_path, operator_profile).unwrap();

    let output = run_install(&bundle, &install_root, Some(&host), &[], &[]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("existing m80 profile would be overwritten"),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!("old_path={}", profile_path.display())),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!("proposed_path={}", profile_path.display())),
        "{stderr}"
    );
    assert!(stderr.contains("--adopt-existing-config"), "{stderr}");
    assert_eq!(fs::read_to_string(&profile_path).unwrap(), operator_profile);
    assert!(!install_root.join("active").exists());
}

#[test]
fn install_bundle_layout_adopts_existing_selector_files_with_explicit_flag() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let profile_path = install_root.join("profiles/default.toml");
    fs::create_dir_all(profile_path.parent().unwrap()).unwrap();
    fs::write(&profile_path, "description = 'operator profile'\n").unwrap();
    fs::write(
        install_root.join("config.toml"),
        "default_profile = 'operator'\nrun_root = '/operator/run'\n",
    )
    .unwrap();

    let output = run_install_with_extra_args(
        &bundle,
        &install_root,
        Some(&host),
        &["--adopt-existing-config"],
        &[],
        &[],
    );

    assert!(
        output.status.success(),
        "adoption install failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let config = fs::read_to_string(install_root.join("config.toml")).unwrap();
    assert!(config.contains("default_profile = 'default'"), "{config}");
    assert!(!config.contains("operator"), "{config}");
    let profile = fs::read_to_string(profile_path).unwrap();
    assert!(
        profile.contains("m80 installed default profile"),
        "{profile}"
    );
    assert!(!profile.contains("operator profile"), "{profile}");
}

#[test]
fn install_bundle_layout_rolls_back_adopted_selector_files_after_late_failure() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let install_temp = tempfile::tempdir().unwrap();
    let install_root = install_temp.path().join("install-root");
    let previous = seed_previous_active_install(&install_root);
    let profile_path = install_root.join("profiles/default.toml");
    fs::create_dir_all(profile_path.parent().unwrap()).unwrap();
    let operator_profile = "description = 'operator profile'\n";
    let operator_config = "default_profile = 'operator'\nrun_root = '/operator/run'\n";
    fs::write(&profile_path, operator_profile).unwrap();
    fs::write(install_root.join("config.toml"), operator_config).unwrap();

    let output = run_install_with_extra_args(
        &bundle,
        &install_root,
        Some(&host),
        &["--adopt-existing-config"],
        &[("PATH", "/usr/bin:/bin")],
        &[],
    );

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("PATH handoff failed"), "{stderr}");
    assert_eq!(fs::read_to_string(&profile_path).unwrap(), operator_profile);
    assert_eq!(
        fs::read_to_string(install_root.join("config.toml")).unwrap(),
        operator_config
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        previous
    );
}
