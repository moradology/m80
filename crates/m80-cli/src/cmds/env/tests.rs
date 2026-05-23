use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn env_json_has_data_version_and_host_sections() {
    let rendered = json::to_pretty(&collect_env_dump());
    let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(parsed["version"], 1);
    assert_eq!(parsed["data"]["version"], 1);
    assert!(parsed["data"]["host"]["kvm"].is_object());
    assert!(parsed["data"]["config"].is_object());
    assert!(parsed["data"]["firecracker"].is_object());
    assert!(parsed["data"]["firecracker"]["seccomp_filter_path"].is_string());
}

#[test]
fn human_output_names_bug_report_fields() {
    let text = render::render_env_human(&collect_env_dump());

    assert!(text.contains("kvm:"));
    assert!(text.contains("firecracker:"));
    assert!(text.contains("firecracker_seccomp_filter:"));
    assert!(text.contains("run_root:"));
    assert!(text.contains("preflight:"));
}

#[test]
fn env_json_reports_selected_installed_profile_paths() {
    let _lock = m80_test_helpers::env::env_lock().lock().unwrap();
    let _restore = m80_test_helpers::env::EnvRestore::capture(&[
        "HOME",
        "M80_DEFAULT_PROFILE",
        "M80_RUN_ROOT",
        "M80_ARTIFACT_DIR",
        "M80_KERNEL_IMAGE",
        "M80_ROOTFS_IMAGE",
        "M80_KERNEL_KIND",
        m80_preflight::ENV_FIRECRACKER_BIN,
        m80_preflight::ENV_FIRECRACKER_SECCOMP_FILTER,
        m80_preflight::ENV_FIRECRACKER_VERSION,
    ]);
    for key in [
        "M80_ARTIFACT_DIR",
        "M80_KERNEL_IMAGE",
        "M80_ROOTFS_IMAGE",
        "M80_KERNEL_KIND",
        m80_preflight::ENV_FIRECRACKER_BIN,
        m80_preflight::ENV_FIRECRACKER_SECCOMP_FILTER,
        m80_preflight::ENV_FIRECRACKER_VERSION,
    ] {
        std::env::remove_var(key);
    }

    let home = tempfile::tempdir().unwrap();
    let profile_dir = home.path().join(".config/m80/profiles");
    let install_root = home.path().join("install");
    let artifacts = install_root.join("versions/v1/artifacts");
    let bin = install_root.join("versions/v1/bin");
    std::fs::create_dir_all(&profile_dir).unwrap();
    std::fs::create_dir_all(&artifacts).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    let firecracker = bin.join("firecracker");
    std::fs::write(&firecracker, "#!/bin/sh\nprintf 'Firecracker v1.15.1\\n'\n").unwrap();
    let mut mode = std::fs::metadata(&firecracker).unwrap().permissions();
    mode.set_mode(0o755);
    std::fs::set_permissions(&firecracker, mode).unwrap();
    let seccomp = bin.join("firecracker-seccomp-filter.bin");
    std::fs::write(&seccomp, "{}\n").unwrap();
    let run_root = home.path().join("run-root");

    std::fs::write(
        profile_dir.join("default.toml"),
        format!(
            "artifact_dir = '{}'\n\
             kernel_image = '{}'\n\
             rootfs_image = '{}'\n\
             kernel_kind = 'stripped'\n\
             firecracker_bin = '{}'\n\
             firecracker_seccomp_filter = '{}'\n\
             jailer_bin = '{}'\n\
             jailer_harden_bin = '{}'\n\
             net_helper_bin = '{}'\n\
             run_root = '{}'\n\
             release_tag = 'v1'\n\
             m80_version = 'v1'\n",
            artifacts.display(),
            artifacts.join("vmlinux").display(),
            artifacts.join("output.ext4").display(),
            firecracker.display(),
            seccomp.display(),
            bin.join("jailer").display(),
            bin.join("m80-jailer-harden").display(),
            bin.join("m80-net-helper").display(),
            run_root.display()
        ),
    )
    .unwrap();
    std::env::set_var("HOME", home.path());
    std::env::set_var("M80_DEFAULT_PROFILE", "default");
    std::env::set_var("M80_RUN_ROOT", &run_root);

    let rendered = json::to_pretty(&collect_env_dump());
    let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();
    let profile = &parsed["data"]["runtime_profile"];

    assert_eq!(profile["name"], "default");
    assert_eq!(profile["selection_source"], "Env");
    assert_eq!(profile["body_source"], "user_file");
    assert_eq!(
        profile["file_path"].as_str(),
        profile_dir.join("default.toml").to_str()
    );
    assert_eq!(profile["artifact_dir"].as_str(), artifacts.to_str());
    assert_eq!(
        profile["kernel_image"].as_str(),
        artifacts.join("vmlinux").to_str()
    );
    assert_eq!(
        profile["rootfs_image"].as_str(),
        artifacts.join("output.ext4").to_str()
    );
    assert_eq!(profile["firecracker_bin"].as_str(), firecracker.to_str());
    assert_eq!(
        profile["firecracker_seccomp_filter"].as_str(),
        seccomp.to_str()
    );
    assert_eq!(profile["run_root"].as_str(), run_root.to_str());
    assert_eq!(
        profile["active_pointer"].as_str(),
        install_root.join("active").to_str()
    );
    assert_eq!(profile["active_pointer_status"], "missing");
    let missing_paths = profile["missing_paths"].as_array().unwrap();
    assert!(
        missing_paths
            .iter()
            .any(|missing| missing["field"] == "rootfs_image"
                && missing["path"] == artifacts.join("output.ext4").to_str().unwrap()
                && missing["reason"] == "missing"),
        "{missing_paths:?}"
    );
    assert_eq!(
        parsed["data"]["artifacts"]["kernel_image"].as_str(),
        artifacts.join("vmlinux").to_str()
    );
    assert_eq!(
        parsed["data"]["artifacts"]["rootfs_image"].as_str(),
        artifacts.join("output.ext4").to_str()
    );
    assert_eq!(
        parsed["data"]["firecracker"]["path"].as_str(),
        firecracker.to_str()
    );
    assert_eq!(
        parsed["data"]["run_root"]["path"].as_str(),
        run_root.to_str()
    );
}
