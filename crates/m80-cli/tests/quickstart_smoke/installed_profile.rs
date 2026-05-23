use super::support::*;
use serde_json::Value;

#[test]
fn quickstart_profile_records_release_tag_from_download_url() {
    let dir = tempfile::tempdir().unwrap();
    let source_tarball = write_release_tarball(&dir);
    let release_dir = dir.path().join("github/releases/download/v9.8.7");
    std::fs::create_dir_all(&release_dir).unwrap();
    let tarball = release_dir.join("m80-linux-x86_64-minimal-artifacts.tar.gz");
    std::fs::copy(&source_tarball, &tarball).unwrap();
    let tarball_sha = sha256_hex(&tarball);
    std::fs::write(
        format!("{}.sha256", tarball.display()),
        format!(
            "{tarball_sha}  {}\n",
            tarball.file_name().unwrap().to_string_lossy()
        ),
    )
    .unwrap();
    let paths = install_paths(&dir, "tagged");

    m80()
        .args(quickstart_no_run_args(&tarball, &paths))
        .assert()
        .success();

    let profile = read_toml(&paths.profile_dir.join("default.toml"));
    assert_eq!(toml_str(&profile, "release_tag"), "v9.8.7");
}

#[test]
fn quickstart_profile_uses_installed_manifest_kernel_kind() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = write_release_tarball_with_kernel_kind(&dir, KernelKind::Stripped);
    let paths = install_paths(&dir, "stripped");

    m80()
        .args(quickstart_no_run_args(&tarball, &paths))
        .assert()
        .success();

    let profile = read_toml(&paths.profile_dir.join("default.toml"));
    assert_eq!(toml_str(&profile, "kernel_kind"), "stripped");
}

#[test]
fn quickstart_rejects_stale_profile_and_config_without_silent_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = write_release_tarball(&dir);
    let paths = install_paths(&dir, "stale");
    std::fs::create_dir_all(&paths.profile_dir).unwrap();
    std::fs::write(paths.profile_dir.join("default.toml"), "stale = true\n").unwrap();
    std::fs::write(
        &paths.config_path,
        "default_profile = \"old\"\nrun_root = \"/old\"\n",
    )
    .unwrap();

    let output = m80()
        .args(quickstart_no_run_args(&tarball, &paths))
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("existing m80 profile would be overwritten"),
        "{stderr}"
    );
    let profile_text = std::fs::read_to_string(paths.profile_dir.join("default.toml")).unwrap();
    assert!(profile_text.contains("stale = true"));
    let config = read_toml(&paths.config_path);
    assert_eq!(toml_str(&config, "default_profile"), "old");
    assert_eq!(toml_str(&config, "run_root"), "/old");
}

#[test]
fn quickstart_json_no_run_keeps_stdout_machine_readable() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = write_release_tarball(&dir);
    let paths = install_paths(&dir, "json");

    let output = m80()
        .arg("--json")
        .args(quickstart_no_run_args(&tarball, &paths))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "quickstart --json failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["version"], 1);
    assert_eq!(value["data"]["ran_probe"], false);
    assert_eq!(
        value["data"]["artifact_dir"].as_str(),
        Some(paths.dst.to_str().unwrap())
    );
    assert_eq!(
        value["data"]["profile_path"].as_str(),
        Some(paths.profile_dir.join("default.toml").to_str().unwrap())
    );
    assert_eq!(
        value["data"]["config_path"].as_str(),
        Some(paths.config_path.to_str().unwrap())
    );
    assert_eq!(
        value["data"]["host_binaries_manifest"].as_str(),
        Some(
            paths
                .dst
                .join("host-binaries.manifest.json")
                .to_str()
                .unwrap()
        )
    );
    assert_eq!(
        value["data"]["host_binaries_manifest_generated"].as_bool(),
        Some(false)
    );
    assert_eq!(
        value["data"]["next_check_command"].as_str(),
        Some("m80 preflight")
    );
    assert_eq!(
        value["data"]["probe_command"].as_str(),
        Some("m80 run -- echo hello")
    );
    assert_eq!(
        value["data"]["probe_egress_policy"].as_str(),
        Some("default-outbound")
    );
}
