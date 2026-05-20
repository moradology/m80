use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{ProfileBodySource, RuntimeProfile};

/// Structured diagnostic view of a resolved runtime profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RuntimeProfileReport {
    pub(crate) name: String,
    pub(crate) selection_source: String,
    pub(crate) body_source: &'static str,
    pub(crate) file_path: Option<PathBuf>,
    pub(crate) artifact_dir: Option<PathBuf>,
    pub(crate) kernel_image: Option<PathBuf>,
    pub(crate) rootfs_image: Option<PathBuf>,
    pub(crate) kernel_kind: Option<String>,
    pub(crate) guestd: Option<PathBuf>,
    pub(crate) guest_manifest: Option<PathBuf>,
    pub(crate) build_receipt: Option<PathBuf>,
    pub(crate) install_provenance: Option<PathBuf>,
    pub(crate) host_binaries_manifest: Option<PathBuf>,
    pub(crate) firecracker_bin: Option<PathBuf>,
    pub(crate) firecracker_seccomp_filter: Option<PathBuf>,
    pub(crate) jailer_bin: Option<PathBuf>,
    pub(crate) jailer_harden_bin: Option<PathBuf>,
    pub(crate) net_helper_bin: Option<PathBuf>,
    pub(crate) run_root: Option<PathBuf>,
    pub(crate) release_tag: Option<String>,
    pub(crate) m80_version: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) active_pointer: Option<PathBuf>,
    pub(crate) active_pointer_target: Option<PathBuf>,
    pub(crate) active_pointer_status: Option<&'static str>,
    pub(crate) active_pointer_error: Option<String>,
    pub(crate) missing_paths: Vec<RuntimeProfilePathIssue>,
}

/// Missing path reported from a resolved profile field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RuntimeProfilePathIssue {
    pub(crate) field: &'static str,
    pub(crate) path: PathBuf,
    pub(crate) reason: &'static str,
}

pub(crate) fn runtime_profile_report(profile: &RuntimeProfile) -> RuntimeProfileReport {
    let active = active_pointer_diagnostic(profile.artifact_dir.as_deref());
    RuntimeProfileReport {
        name: profile.name.clone(),
        selection_source: format!("{:?}", profile.selection_source),
        body_source: profile_body_source_label(profile.body_source),
        file_path: profile.file_path.clone(),
        artifact_dir: profile.artifact_dir.clone(),
        kernel_image: profile.kernel_image.clone(),
        rootfs_image: profile.rootfs_image.clone(),
        kernel_kind: profile.kernel_kind.clone(),
        guestd: profile.guestd.clone(),
        guest_manifest: profile.guest_manifest.clone(),
        build_receipt: profile.build_receipt.clone(),
        install_provenance: profile.install_provenance.clone(),
        host_binaries_manifest: profile.host_binaries_manifest.clone(),
        firecracker_bin: profile.firecracker_bin.clone(),
        firecracker_seccomp_filter: profile.firecracker_seccomp_filter.clone(),
        jailer_bin: profile.jailer_bin.clone(),
        jailer_harden_bin: profile.jailer_harden_bin.clone(),
        net_helper_bin: profile.net_helper_bin.clone(),
        run_root: profile.run_root.clone(),
        release_tag: profile.release_tag.clone(),
        m80_version: profile.m80_version.clone(),
        description: profile.description.clone(),
        active_pointer: active.pointer,
        active_pointer_target: active.target,
        active_pointer_status: active.status,
        active_pointer_error: active.error,
        missing_paths: missing_paths(profile),
    }
}

fn profile_body_source_label(source: ProfileBodySource) -> &'static str {
    match source {
        ProfileBodySource::BuiltinEnv => "builtin_env",
        ProfileBodySource::SystemFile => "system_file",
        ProfileBodySource::UserFile => "user_file",
    }
}

struct ActivePointerDiagnostic {
    pointer: Option<PathBuf>,
    target: Option<PathBuf>,
    status: Option<&'static str>,
    error: Option<String>,
}

fn active_pointer_diagnostic(artifact_dir: Option<&Path>) -> ActivePointerDiagnostic {
    let Some(artifact_dir) = artifact_dir else {
        return empty_active_pointer();
    };
    let Some(version_dir) = artifact_dir.parent() else {
        return empty_active_pointer();
    };
    let Some(versions_dir) = version_dir.parent() else {
        return empty_active_pointer();
    };
    if versions_dir.file_name().and_then(|name| name.to_str()) != Some("versions") {
        return empty_active_pointer();
    }
    let Some(install_root) = versions_dir.parent() else {
        return empty_active_pointer();
    };
    let pointer = install_root.join("active");
    match std::fs::read_link(&pointer) {
        Ok(target) => {
            let resolved_target = if target.is_absolute() {
                target.clone()
            } else {
                pointer
                    .parent()
                    .expect("active pointer path has install root parent")
                    .join(&target)
            };
            let status = if paths_refer_to_same_location(&resolved_target, version_dir) {
                "live"
            } else {
                "stale"
            };
            ActivePointerDiagnostic {
                pointer: Some(pointer),
                target: Some(target),
                status: Some(status),
                error: None,
            }
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => ActivePointerDiagnostic {
            pointer: Some(pointer),
            target: None,
            status: Some("missing"),
            error: None,
        },
        Err(source) => ActivePointerDiagnostic {
            pointer: Some(pointer),
            target: None,
            status: Some("error"),
            error: Some(source.to_string()),
        },
    }
}

fn empty_active_pointer() -> ActivePointerDiagnostic {
    ActivePointerDiagnostic {
        pointer: None,
        target: None,
        status: None,
        error: None,
    }
}

fn paths_refer_to_same_location(left: &Path, right: &Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn missing_paths(profile: &RuntimeProfile) -> Vec<RuntimeProfilePathIssue> {
    let mut missing = Vec::new();
    for (field, path) in [
        ("artifact_dir", profile.artifact_dir.as_deref()),
        ("kernel_image", profile.kernel_image.as_deref()),
        ("rootfs_image", profile.rootfs_image.as_deref()),
        ("guestd", profile.guestd.as_deref()),
        ("guest_manifest", profile.guest_manifest.as_deref()),
        ("build_receipt", profile.build_receipt.as_deref()),
        ("install_provenance", profile.install_provenance.as_deref()),
        (
            "host_binaries_manifest",
            profile.host_binaries_manifest.as_deref(),
        ),
        ("firecracker_bin", profile.firecracker_bin.as_deref()),
        (
            "firecracker_seccomp_filter",
            profile.firecracker_seccomp_filter.as_deref(),
        ),
        ("jailer_bin", profile.jailer_bin.as_deref()),
        ("jailer_harden_bin", profile.jailer_harden_bin.as_deref()),
        ("net_helper_bin", profile.net_helper_bin.as_deref()),
        ("run_root", profile.run_root.as_deref()),
    ] {
        if let Some(path) = path {
            if !path.exists() {
                missing.push(RuntimeProfilePathIssue {
                    field,
                    path: path.to_path_buf(),
                    reason: "missing",
                });
            }
        }
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;
    use m80_firecracker::ConfigSource;

    fn diagnostic_profile(artifact_dir: PathBuf) -> RuntimeProfile {
        RuntimeProfile {
            name: "default".to_owned(),
            selection_source: ConfigSource::SystemFile,
            body_source: ProfileBodySource::SystemFile,
            file_path: None,
            artifact_dir: Some(artifact_dir.clone()),
            kernel_image: Some(artifact_dir.join("vmlinux")),
            rootfs_image: Some(artifact_dir.join("output.ext4")),
            kernel_kind: Some("stripped".to_owned()),
            guestd: Some(artifact_dir.join("m80-guestd")),
            guest_manifest: Some(artifact_dir.join("output.ext4.manifest.json")),
            build_receipt: Some(artifact_dir.join("output.ext4.build-receipt.json")),
            install_provenance: Some(artifact_dir.join("install-provenance.json")),
            host_binaries_manifest: Some(artifact_dir.join("host-binaries.manifest.json")),
            firecracker_bin: None,
            firecracker_seccomp_filter: None,
            jailer_bin: None,
            jailer_harden_bin: None,
            net_helper_bin: None,
            run_root: None,
            release_tag: Some("v1".to_owned()),
            m80_version: Some("v1".to_owned()),
            description: Some("m80 installed default profile".to_owned()),
        }
    }

    #[test]
    fn report_marks_live_active_pointer_for_installed_layout() {
        let dir = tempfile::tempdir().unwrap();
        let version_dir = dir.path().join("versions/v1");
        let artifact_dir = version_dir.join("artifacts");
        std::fs::create_dir_all(&artifact_dir).unwrap();
        std::os::unix::fs::symlink(&version_dir, dir.path().join("active")).unwrap();

        let report = runtime_profile_report(&diagnostic_profile(artifact_dir));

        assert_eq!(
            report.active_pointer.as_deref(),
            Some(dir.path().join("active").as_path())
        );
        assert_eq!(report.active_pointer_status, Some("live"));
        assert_eq!(
            report.active_pointer_target.as_deref(),
            Some(version_dir.as_path())
        );
    }

    #[test]
    fn report_marks_stale_active_pointer_for_other_version() {
        let dir = tempfile::tempdir().unwrap();
        let version_dir = dir.path().join("versions/v1");
        let other_version_dir = dir.path().join("versions/v0");
        let artifact_dir = version_dir.join("artifacts");
        std::fs::create_dir_all(&artifact_dir).unwrap();
        std::fs::create_dir_all(&other_version_dir).unwrap();
        std::os::unix::fs::symlink(&other_version_dir, dir.path().join("active")).unwrap();

        let report = runtime_profile_report(&diagnostic_profile(artifact_dir));

        assert_eq!(report.active_pointer_status, Some("stale"));
        assert_eq!(
            report.active_pointer_target.as_deref(),
            Some(other_version_dir.as_path())
        );
    }

    #[test]
    fn report_names_missing_profile_paths_by_field() {
        let dir = tempfile::tempdir().unwrap();
        let version_dir = dir.path().join("versions/v1");
        let artifact_dir = version_dir.join("artifacts");
        std::fs::create_dir_all(&artifact_dir).unwrap();
        std::fs::write(artifact_dir.join("vmlinux"), b"kernel").unwrap();

        let report = runtime_profile_report(&diagnostic_profile(artifact_dir));

        assert!(report
            .missing_paths
            .iter()
            .any(|missing| missing.field == "rootfs_image"
                && missing.path.ends_with("output.ext4")
                && missing.reason == "missing"));
        assert!(!report
            .missing_paths
            .iter()
            .any(|missing| missing.field == "kernel_image"));
    }
}
