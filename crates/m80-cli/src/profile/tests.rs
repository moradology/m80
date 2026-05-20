use std::path::{Path, PathBuf};

use m80_firecracker::{ConfigSource, EffectiveConfig, EffectiveField};
use tempfile::TempDir;

use super::*;

fn effective_profile(name: &str, source: ConfigSource) -> EffectiveConfig {
    EffectiveConfig {
        fields: vec![EffectiveField {
            name: DEFAULT_PROFILE_FIELD.to_owned(),
            value: name.to_owned(),
            source,
        }],
    }
}

fn paths(system: &TempDir, user: &TempDir) -> ProfileFilePaths {
    ProfileFilePaths {
        system_dir: Some(system.path().to_path_buf()),
        user_dir: Some(user.path().to_path_buf()),
    }
}

fn write_profile(dir: &TempDir, name: &str, content: &str) {
    std::fs::write(dir.path().join(profile_filename(name)), content).unwrap();
}

#[test]
fn default_env_profile_requires_no_profile_file() {
    let system = tempfile::tempdir().unwrap();
    let user = tempfile::tempdir().unwrap();

    let profile = resolve_from_effective(
        &effective_profile("env", ConfigSource::Default),
        paths(&system, &user),
    )
    .unwrap();

    assert_eq!(profile.name, "env");
    assert_eq!(profile.selection_source, ConfigSource::Default);
    assert_eq!(profile.body_source, ProfileBodySource::BuiltinEnv);
    assert!(profile.file_path.is_none());
    assert!(profile.artifact_dir.is_none());
    assert!(profile.kernel_image.is_none());
    assert!(profile.rootfs_image.is_none());
    assert!(profile.run_root.is_none());
}

#[test]
fn explicit_profile_loads_user_file_over_system_file() {
    let system = tempfile::tempdir().unwrap();
    let user = tempfile::tempdir().unwrap();
    write_profile(
        &system,
        "dev",
        "kernel_image = \"/system/vmlinux\"\nrootfs_image = \"/system/rootfs.ext4\"\n",
    );
    write_profile(
        &user,
        "dev",
        "artifact_dir = \"/user/artifacts\"\nkernel_image = \"/user/vmlinux\"\nrootfs_image = \"/user/rootfs.ext4\"\nkernel_kind = \"stripped\"\nguestd = \"/user/m80-guestd\"\nguest_manifest = \"/user/output.ext4.manifest.json\"\nbuild_receipt = \"/user/output.ext4.build-receipt.json\"\ninstall_provenance = \"/user/install-provenance.json\"\nhost_binaries_manifest = \"/user/host-binaries.manifest.json\"\nfirecracker_bin = \"/opt/firecracker/bin/firecracker\"\nfirecracker_seccomp_filter = \"/opt/firecracker/bin/firecracker-seccomp-filter.bin\"\njailer_bin = \"/opt/firecracker/bin/jailer\"\njailer_harden_bin = \"/opt/m80/bin/m80-jailer-harden\"\nnet_helper_bin = \"/opt/m80/bin/m80-net-helper\"\nrun_root = \"/run/m80\"\nrelease_tag = \"v1.2.3\"\nm80_version = \"v1.2.3\"\ndescription = \"developer shell\"\n",
    );

    let profile = resolve_from_effective(
        &effective_profile("dev", ConfigSource::Flag),
        paths(&system, &user),
    )
    .unwrap();

    assert_eq!(profile.name, "dev");
    assert_eq!(profile.selection_source, ConfigSource::Flag);
    assert_eq!(profile.body_source, ProfileBodySource::UserFile);
    assert_eq!(
        profile.artifact_dir.as_deref(),
        Some(Path::new("/user/artifacts"))
    );
    assert_eq!(
        profile.kernel_image.as_deref(),
        Some(Path::new("/user/vmlinux"))
    );
    assert_eq!(
        profile.rootfs_image.as_deref(),
        Some(Path::new("/user/rootfs.ext4"))
    );
    assert_eq!(profile.kernel_kind.as_deref(), Some("stripped"));
    assert_eq!(
        profile.guestd.as_deref(),
        Some(Path::new("/user/m80-guestd"))
    );
    assert_eq!(
        profile.guest_manifest.as_deref(),
        Some(Path::new("/user/output.ext4.manifest.json"))
    );
    assert_eq!(
        profile.build_receipt.as_deref(),
        Some(Path::new("/user/output.ext4.build-receipt.json"))
    );
    assert_eq!(
        profile.install_provenance.as_deref(),
        Some(Path::new("/user/install-provenance.json"))
    );
    assert_eq!(
        profile.host_binaries_manifest.as_deref(),
        Some(Path::new("/user/host-binaries.manifest.json"))
    );
    assert_eq!(
        profile.firecracker_bin.as_deref(),
        Some(Path::new("/opt/firecracker/bin/firecracker"))
    );
    assert_eq!(
        profile.firecracker_seccomp_filter.as_deref(),
        Some(Path::new(
            "/opt/firecracker/bin/firecracker-seccomp-filter.bin"
        ))
    );
    assert_eq!(
        profile.jailer_bin.as_deref(),
        Some(Path::new("/opt/firecracker/bin/jailer"))
    );
    assert_eq!(
        profile.jailer_harden_bin.as_deref(),
        Some(Path::new("/opt/m80/bin/m80-jailer-harden"))
    );
    assert_eq!(
        profile.net_helper_bin.as_deref(),
        Some(Path::new("/opt/m80/bin/m80-net-helper"))
    );
    assert_eq!(profile.run_root.as_deref(), Some(Path::new("/run/m80")));
    assert_eq!(profile.release_tag.as_deref(), Some("v1.2.3"));
    assert_eq!(profile.m80_version.as_deref(), Some("v1.2.3"));
    assert_eq!(profile.description.as_deref(), Some("developer shell"));
}

#[test]
fn missing_profile_fails_before_preflight() {
    let system = tempfile::tempdir().unwrap();
    let user = tempfile::tempdir().unwrap();

    let err = resolve_from_effective(
        &effective_profile("missing", ConfigSource::Flag),
        paths(&system, &user),
    )
    .unwrap_err();

    assert!(
        err.to_string()
            .contains("runtime profile \"missing\" not found"),
        "unexpected error: {err}"
    );
}

#[test]
fn unknown_profile_keys_fail_closed() {
    let system = tempfile::tempdir().unwrap();
    let user = tempfile::tempdir().unwrap();
    write_profile(
        &user,
        "dev",
        "kernel_image = \"/vmlinux\"\nrootfs_image = \"/rootfs.ext4\"\nextra = true\n",
    );

    let err = resolve_from_effective(
        &effective_profile("dev", ConfigSource::Flag),
        paths(&system, &user),
    )
    .unwrap_err();

    assert!(
        err.to_string().contains("unknown field `extra`"),
        "unexpected error: {err}"
    );
}

#[test]
fn invalid_kernel_kind_fails_closed() {
    let system = tempfile::tempdir().unwrap();
    let user = tempfile::tempdir().unwrap();
    write_profile(
        &user,
        "dev",
        "kernel_image = \"/vmlinux\"\nrootfs_image = \"/rootfs.ext4\"\nkernel_kind = \"tiny\"\n",
    );

    let err = resolve_from_effective(
        &effective_profile("dev", ConfigSource::Flag),
        paths(&system, &user),
    )
    .unwrap_err();

    assert!(
        err.to_string()
            .contains("kernel_kind must be stock|stripped"),
        "unexpected error: {err}"
    );
}

#[test]
fn path_like_profile_names_fail_closed() {
    let system = tempfile::tempdir().unwrap();
    let user = tempfile::tempdir().unwrap();

    let err = resolve_from_effective(
        &effective_profile("../dev", ConfigSource::Flag),
        paths(&system, &user),
    )
    .unwrap_err();

    assert!(
        err.to_string().contains("single path segment"),
        "unexpected error: {err}"
    );
}

#[test]
fn profile_paths_are_deterministic() {
    let paths = ProfileFilePaths {
        system_dir: Some(PathBuf::from("/etc/m80/profiles")),
        user_dir: Some(PathBuf::from("/home/alice/.config/m80/profiles")),
    };

    assert_eq!(
        paths.search_paths("dev"),
        vec![
            PathBuf::from("/etc/m80/profiles/dev.toml"),
            PathBuf::from("/home/alice/.config/m80/profiles/dev.toml"),
        ]
    );
}
