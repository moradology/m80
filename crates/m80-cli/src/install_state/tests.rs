use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use m80_firecracker::{ConfigFilePaths, ConfigSource};

use super::*;

#[test]
fn host_paths_use_documented_linux_locations() {
    let paths = InstallStatePaths::host("/opt/m80");

    assert_eq!(paths.install_root, PathBuf::from("/opt/m80"));
    assert_eq!(
        paths.config_paths.system.as_deref(),
        Some(Path::new("/etc/m80/config.toml"))
    );
    assert_eq!(
        paths.profile_paths.system_dir.as_deref(),
        Some(Path::new("/etc/m80/profiles"))
    );
}

#[test]
fn resolver_reports_healthy_active_release() {
    let fixture = InstallStateFixture::new();
    fixture.write_installed_profile("v1.2.3");
    fixture.write_system_config("default");
    fixture.point_active_at("v1.2.3");

    let report = fixture.resolve(None);

    assert_eq!(report.state, InstallStateKind::HealthyActiveRelease);
    assert_eq!(report.active_pointer.status, ActivePointerStatus::Live);
    assert_eq!(report.active_pointer.release_tag.as_deref(), Some("v1.2.3"));
    assert_eq!(report.config.default_profile.as_deref(), Some("default"));
    assert_eq!(
        report.config.default_profile_source,
        Some(ConfigSource::SystemFile)
    );
    assert_eq!(
        report
            .profile
            .as_ref()
            .and_then(|profile| profile.release_tag.as_deref()),
        Some("v1.2.3")
    );
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
}

#[test]
fn resolver_reports_missing_active_pointer() {
    let fixture = InstallStateFixture::new();
    fixture.write_installed_profile("v1.2.3");
    fixture.write_system_config("default");

    let report = fixture.resolve(None);

    assert_eq!(report.state, InstallStateKind::MissingActivePointer);
    assert_eq!(report.active_pointer.status, ActivePointerStatus::Missing);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == InstallStateDiagnosticCode::MissingActivePointer));
}

#[test]
fn resolver_reports_dangling_active_pointer() {
    let fixture = InstallStateFixture::new();
    fixture.write_installed_profile("v1.2.3");
    fixture.write_system_config("default");
    symlink(
        fixture.install_root().join("versions/v9.9.9"),
        fixture.install_root().join("active"),
    )
    .unwrap();

    let report = fixture.resolve(None);

    assert_eq!(report.state, InstallStateKind::DanglingActivePointer);
    assert_eq!(report.active_pointer.status, ActivePointerStatus::Dangling);
    assert_eq!(report.active_pointer.release_tag.as_deref(), Some("v9.9.9"));
}

#[test]
fn resolver_reports_local_dev_tree_from_builtin_env_default() {
    let fixture = InstallStateFixture::new();

    let report = fixture.resolve(None);

    assert_eq!(report.state, InstallStateKind::LocalDevTree);
    assert_eq!(
        report.profile.as_ref().map(|profile| profile.body_source),
        Some("builtin_env")
    );
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == InstallStateDiagnosticCode::LocalDevProfile));
}

#[test]
fn resolver_reports_stale_profile_target() {
    let fixture = InstallStateFixture::new();
    fixture.write_installed_profile("v1.2.3");
    fixture.write_version_dir("v1.2.4");
    fixture.write_system_config("default");
    fixture.point_active_at("v1.2.4");

    let report = fixture.resolve(None);

    assert_eq!(report.state, InstallStateKind::StaleProfileTarget);
    assert_eq!(report.active_pointer.release_tag.as_deref(), Some("v1.2.4"));
    assert_eq!(
        report
            .profile
            .as_ref()
            .and_then(|profile| profile.release_tag.as_deref()),
        Some("v1.2.3")
    );
}

#[test]
fn resolver_reports_explicit_env_profile_override() {
    let fixture = InstallStateFixture::new();
    fixture.write_installed_profile("v1.2.3");
    fixture.write_system_config("default");
    fixture.point_active_at("v1.2.3");
    std::env::set_var("M80_DEFAULT_PROFILE", "env");

    let report = fixture.resolve(None);

    assert_eq!(report.state, InstallStateKind::ExplicitOverride);
    assert_eq!(report.config.default_profile.as_deref(), Some("env"));
    assert_eq!(
        report.config.default_profile_source,
        Some(ConfigSource::Env)
    );
    assert!(report.config.explicit_override);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == InstallStateDiagnosticCode::ExplicitProfileOverride));
}

#[test]
fn resolver_reports_explicit_flag_profile_override() {
    let fixture = InstallStateFixture::new();
    fixture.write_installed_profile("v1.2.3");
    fixture.write_named_profile("other", "v9.0.0");
    fixture.write_system_config("default");
    fixture.point_active_at("v1.2.3");

    let report = fixture.resolve(Some("other".to_owned()));

    assert_eq!(report.state, InstallStateKind::ExplicitOverride);
    assert_eq!(report.config.default_profile.as_deref(), Some("other"));
    assert_eq!(
        report.config.default_profile_source,
        Some(ConfigSource::Flag)
    );
    assert_eq!(
        report.profile.as_ref().map(|profile| profile.name.as_str()),
        Some("other")
    );
}

#[test]
fn resolver_rejects_active_pointer_traversal() {
    let fixture = InstallStateFixture::new();
    fixture.write_installed_profile("v1.2.3");
    fixture.write_system_config("default");
    symlink("../outside", fixture.install_root().join("active")).unwrap();

    let report = fixture.resolve(None);

    assert_eq!(report.state, InstallStateKind::InvalidInstallMetadata);
    assert_eq!(report.active_pointer.status, ActivePointerStatus::Invalid);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == InstallStateDiagnosticCode::ActivePointerTraversal));
}

#[test]
fn resolver_rejects_profile_path_outside_install_root() {
    let fixture = InstallStateFixture::new();
    fixture.write_system_config("default");
    fixture.point_active_at("v1.2.3");
    fixture.write_profile_with_artifact_dir(
        "default",
        "v1.2.3",
        fixture.temp.path().join("elsewhere/artifacts"),
    );

    let report = fixture.resolve(None);

    assert_eq!(report.state, InstallStateKind::InvalidInstallMetadata);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == InstallStateDiagnosticCode::ProfilePathOutsideInstallRoot
            && diagnostic.field == Some("artifact_dir")
    }));
}

#[test]
fn resolver_rejects_profile_path_traversal() {
    let fixture = InstallStateFixture::new();
    fixture.write_system_config("default");
    fixture.point_active_at("v1.2.3");
    let artifact_dir = fixture
        .install_root()
        .join("versions/v1.2.3/../escape/artifacts");
    fixture.write_profile_with_artifact_dir("default", "v1.2.3", artifact_dir);

    let report = fixture.resolve(None);

    assert_eq!(report.state, InstallStateKind::InvalidInstallMetadata);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == InstallStateDiagnosticCode::ProfilePathTraversal));
}

struct InstallStateFixture {
    temp: tempfile::TempDir,
    _env_restore: m80_test_helpers::env::EnvRestore,
    _env_lock: std::sync::MutexGuard<'static, ()>,
}

impl InstallStateFixture {
    fn new() -> Self {
        let env_lock = m80_test_helpers::env::env_lock().lock().unwrap();
        let env_restore = m80_test_helpers::env::EnvRestore::capture(&[
            "M80_DEFAULT_PROFILE",
            "M80_RUN_ROOT",
            "M80_MAX_CONCURRENT_VMS",
            "M80_JAIL_UID",
            "M80_JAIL_GID",
            "M80_CGROUP_MODE",
        ]);
        for key in [
            "M80_DEFAULT_PROFILE",
            "M80_RUN_ROOT",
            "M80_MAX_CONCURRENT_VMS",
            "M80_JAIL_UID",
            "M80_JAIL_GID",
            "M80_CGROUP_MODE",
        ] {
            std::env::remove_var(key);
        }
        Self {
            temp: tempfile::tempdir().unwrap(),
            _env_restore: env_restore,
            _env_lock: env_lock,
        }
    }

    fn install_root(&self) -> PathBuf {
        self.temp.path().join("install")
    }

    fn profile_dir(&self) -> PathBuf {
        self.temp.path().join("profiles")
    }

    fn config_path(&self) -> PathBuf {
        self.temp.path().join("config.toml")
    }

    fn resolve(&self, profile_override: Option<String>) -> InstallStateReport {
        resolve_install_state(InstallStateRequest {
            paths: InstallStatePaths {
                install_root: self.install_root(),
                config_paths: ConfigFilePaths {
                    system: Some(self.config_path()),
                    system_drop_in_dir: None,
                    user: None,
                    user_drop_in_dir: None,
                },
                profile_paths: ProfileFilePaths {
                    system_dir: Some(self.profile_dir()),
                    user_dir: None,
                },
            },
            profile_override,
        })
    }

    fn write_system_config(&self, profile: &str) {
        fs::write(
            self.config_path(),
            format!("default_profile = '{profile}'\n"),
        )
        .unwrap();
    }

    fn write_installed_profile(&self, tag: &str) {
        self.write_named_profile("default", tag);
    }

    fn write_named_profile(&self, name: &str, tag: &str) {
        let artifact_dir = self
            .install_root()
            .join("versions")
            .join(tag)
            .join("artifacts");
        self.write_profile_with_artifact_dir(name, tag, artifact_dir);
    }

    fn write_profile_with_artifact_dir(&self, name: &str, tag: &str, artifact_dir: PathBuf) {
        fs::create_dir_all(self.profile_dir()).unwrap();
        if artifact_dir.starts_with(self.install_root()) && !path_contains_parent(&artifact_dir) {
            fs::create_dir_all(&artifact_dir).unwrap();
            fs::create_dir_all(
                artifact_dir
                    .parent()
                    .expect("artifact dir should have version parent")
                    .join("bin"),
            )
            .unwrap();
        }
        let version_dir = artifact_dir
            .parent()
            .unwrap_or_else(|| Path::new("/missing-version-dir"));
        let bin_dir = version_dir.join("bin");
        let host_prereq_dir = self.temp.path().join("host-prereqs");
        fs::write(
            self.profile_dir().join(format!("{name}.toml")),
            format!(
                "artifact_dir = '{}'\n\
                 kernel_image = '{}'\n\
                 rootfs_image = '{}'\n\
                 kernel_kind = 'stripped'\n\
                 guestd = '{}'\n\
                 guest_manifest = '{}'\n\
                 build_receipt = '{}'\n\
                 install_provenance = '{}'\n\
                 host_binaries_manifest = '{}'\n\
                 firecracker_bin = '{}'\n\
                 firecracker_seccomp_filter = '{}'\n\
                 jailer_bin = '{}'\n\
                 jailer_harden_bin = '{}'\n\
                 net_helper_bin = '{}'\n\
                 release_tag = '{}'\n\
                 m80_version = '{}'\n",
                artifact_dir.display(),
                artifact_dir.join("vmlinux").display(),
                artifact_dir.join("output.ext4").display(),
                artifact_dir.join("m80-guestd").display(),
                artifact_dir.join("output.ext4.manifest.json").display(),
                artifact_dir
                    .join("output.ext4.build-receipt.json")
                    .display(),
                artifact_dir.join("install-provenance.json").display(),
                artifact_dir.join("host-binaries.manifest.json").display(),
                host_prereq_dir.join("firecracker").display(),
                host_prereq_dir
                    .join("firecracker-seccomp-filter.bin")
                    .display(),
                host_prereq_dir.join("jailer").display(),
                bin_dir.join("m80-jailer-harden").display(),
                bin_dir.join("m80-net-helper").display(),
                tag,
                tag
            ),
        )
        .unwrap();
    }

    fn write_version_dir(&self, tag: &str) {
        fs::create_dir_all(self.install_root().join("versions").join(tag)).unwrap();
    }

    fn point_active_at(&self, tag: &str) {
        let version_dir = self.install_root().join("versions").join(tag);
        fs::create_dir_all(&version_dir).unwrap();
        symlink(version_dir, self.install_root().join("active")).unwrap();
    }
}

fn path_contains_parent(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
}
