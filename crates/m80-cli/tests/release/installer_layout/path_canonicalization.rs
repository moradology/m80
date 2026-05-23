use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::fixture::{write_release_bundle, RELEASE_TAG};
use super::{clear_install_env, m80, path_with_install_bin_first, HostPrereqFixture};

#[test]
fn relative_install_root_is_normalized_before_state_is_written() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let temp = tempfile::tempdir().unwrap();
    let expected_root = temp.path().join("relative-root");

    let output = run_install_from(
        temp.path(),
        &bundle.tarball,
        Path::new("relative-root"),
        Some(&host),
    );

    assert!(
        output.status.success(),
        "install failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let version_dir = expected_root.join("versions").join(RELEASE_TAG);
    assert!(
        stdout.contains(&format!("install_root={}", expected_root.display())),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("active_version_dir={}", version_dir.display())),
        "{stdout}"
    );
    assert_eq!(
        fs::read_link(expected_root.join("active")).unwrap(),
        version_dir
    );

    let profile_path = expected_root.join("profiles/default.toml");
    let profile = fs::read_to_string(&profile_path).unwrap();
    for required in [
        version_dir.join("artifacts").display().to_string(),
        version_dir
            .join("artifacts/output.ext4")
            .display()
            .to_string(),
        version_dir
            .join("artifacts/host-binaries.manifest.json")
            .display()
            .to_string(),
        expected_root.join("run").display().to_string(),
    ] {
        assert!(Path::new(&required).is_absolute(), "{required}");
        assert!(
            profile.contains(&required),
            "profile missing absolute path {required}:\n{profile}"
        );
    }
    for path in [
        version_dir.join("artifacts/output.ext4.manifest.json"),
        version_dir.join("artifacts/output.ext4.build-receipt.json"),
        version_dir.join("artifacts/install-provenance.json"),
        version_dir.join("artifacts/host-binaries.manifest.json"),
    ] {
        let value = serde_json::from_str::<Value>(&fs::read_to_string(&path).unwrap())
            .unwrap_or_else(|err| panic!("parse {}: {err}", path.display()));
        assert_json_path_fields_are_absolute(&value, &path);
    }
}

#[test]
fn symlinked_install_root_fails_before_install_state_is_written() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let temp = tempfile::tempdir().unwrap();
    let real_root = temp.path().join("real-root");
    let link_root = temp.path().join("link-root");
    fs::create_dir(&real_root).unwrap();
    symlink(&real_root, &link_root).unwrap();

    let output = run_install_from(temp.path(), &bundle.tarball, &link_root, Some(&host));

    assert!(
        !output.status.success(),
        "symlinked root unexpectedly installed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("install root must not pass through a symlink"),
        "{stderr}"
    );
    assert!(
        fs::read_dir(&real_root).unwrap().next().is_none(),
        "failed install must not write through symlinked install root"
    );
}

#[test]
fn install_root_symlink_ancestor_fails_before_state_is_written() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let temp = tempfile::tempdir().unwrap();
    let real_parent = temp.path().join("real-parent");
    let link_parent = temp.path().join("link-parent");
    fs::create_dir(&real_parent).unwrap();
    symlink(&real_parent, &link_parent).unwrap();
    let install_root = link_parent.join("install-root");

    let output = run_install_from(temp.path(), &bundle.tarball, &install_root, Some(&host));

    assert!(
        !output.status.success(),
        "symlink ancestor unexpectedly installed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("install root must not pass through a symlink"),
        "{stderr}"
    );
    assert!(
        fs::read_dir(&real_parent).unwrap().next().is_none(),
        "failed install must not write through symlinked install-root ancestor"
    );
}

#[test]
fn relative_existing_active_pointer_fails_before_new_version_is_written() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    fs::create_dir_all(install_root.join("versions/old")).unwrap();
    symlink("versions/old", install_root.join("active")).unwrap();

    let output = run_install_from(temp.path(), &bundle.tarball, &install_root, Some(&host));

    assert!(
        !output.status.success(),
        "relative active pointer unexpectedly installed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("active pointer target must be absolute"),
        "{stderr}"
    );
    assert!(
        !install_root.join("versions").join(RELEASE_TAG).exists(),
        "failed install must not publish a new version directory"
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        PathBuf::from("versions/old")
    );
}

#[test]
fn dangling_existing_active_pointer_fails_before_new_version_is_written() {
    let bundle = write_release_bundle(None);
    let host = HostPrereqFixture::new();
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    fs::create_dir_all(install_root.join("versions")).unwrap();
    let missing_target = install_root.join("versions/missing");
    symlink(&missing_target, install_root.join("active")).unwrap();

    let output = run_install_from(temp.path(), &bundle.tarball, &install_root, Some(&host));

    assert!(
        !output.status.success(),
        "dangling active pointer unexpectedly installed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("active pointer target is stale or missing"),
        "{stderr}"
    );
    assert!(
        !install_root.join("versions").join(RELEASE_TAG).exists(),
        "failed install must not publish a new version directory"
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        missing_target
    );
}

fn run_install_from(
    cwd: &Path,
    tarball: &Path,
    install_root: &Path,
    host: Option<&HostPrereqFixture>,
) -> std::process::Output {
    let mut command = m80();
    command.current_dir(cwd);
    command.args([
        "install",
        "--bundle-url",
        &format!("file://{}", tarball.display()),
        "--install-root",
        install_root.to_str().unwrap(),
    ]);
    clear_install_env(&mut command);
    let effective_root = if install_root.is_absolute() {
        install_root.to_path_buf()
    } else {
        cwd.join(install_root)
    };
    command.env("PATH", path_with_install_bin_first(&effective_root));
    if let Some(host) = host {
        host.apply(&mut command);
    }
    command.output().unwrap()
}

fn assert_json_path_fields_are_absolute(value: &Value, file: &Path) {
    assert_json_path_fields_are_absolute_at(value, file, "$");
}

fn assert_json_path_fields_are_absolute_at(value: &Value, file: &Path, location: &str) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                let child_location = format!("{location}.{key}");
                if is_path_key(key) {
                    assert_json_string_is_absolute_path(child, file, &child_location);
                }
                assert_json_path_fields_are_absolute_at(child, file, &child_location);
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                assert_json_path_fields_are_absolute_at(
                    child,
                    file,
                    &format!("{location}[{index}]"),
                );
            }
        }
        _ => {}
    }
}

fn is_path_key(key: &str) -> bool {
    key != "source_path"
        && (key == "path"
            || key.ends_with("_path")
            || matches!(
                key,
                "kernel_image"
                    | "output_rootfs_image"
                    | "daemon_binary_path"
                    | "artifact_dir"
                    | "run_root"
                    | "firecracker_bin"
                    | "firecracker_seccomp_filter"
                    | "jailer_bin"
                    | "jailer_harden_bin"
                    | "net_helper_bin"
            ))
}

fn assert_json_string_is_absolute_path(value: &Value, file: &Path, location: &str) {
    match value {
        Value::String(path) => {
            assert!(
                Path::new(path).is_absolute(),
                "{} field {location} must be absolute, got {path:?}",
                file.display()
            );
        }
        Value::Null => {}
        other => panic!(
            "{} field {location} should be a path string or null, got {other:?}",
            file.display()
        ),
    }
}
