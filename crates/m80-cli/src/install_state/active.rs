use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{
    diagnostic, path_has_parent_component, release_tag_from_version_dir, InstallStateDiagnostic,
    InstallStateDiagnosticCode, VERSIONS_DIR_NAME,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ActivePointerReport {
    pub(crate) path: PathBuf,
    pub(crate) target: Option<PathBuf>,
    pub(crate) version_dir: Option<PathBuf>,
    pub(crate) release_tag: Option<String>,
    pub(crate) status: ActivePointerStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ActivePointerStatus {
    Live,
    Missing,
    Dangling,
    Invalid,
}

pub(super) fn read_active_pointer(
    install_root: &Path,
    pointer: &Path,
    diagnostics: &mut Vec<InstallStateDiagnostic>,
) -> ActivePointerReport {
    match std::fs::read_link(pointer) {
        Ok(target) => active_pointer_target_report(install_root, pointer, target, diagnostics),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            diagnostics.push(diagnostic(
                InstallStateDiagnosticCode::MissingActivePointer,
                Some("active_pointer"),
                Some(pointer.to_path_buf()),
                "active install pointer is missing".to_owned(),
            ));
            ActivePointerReport {
                path: pointer.to_path_buf(),
                target: None,
                version_dir: None,
                release_tag: None,
                status: ActivePointerStatus::Missing,
            }
        }
        Err(source) => {
            diagnostics.push(diagnostic(
                InstallStateDiagnosticCode::ActivePointerUnreadable,
                Some("active_pointer"),
                Some(pointer.to_path_buf()),
                source.to_string(),
            ));
            ActivePointerReport {
                path: pointer.to_path_buf(),
                target: None,
                version_dir: None,
                release_tag: None,
                status: ActivePointerStatus::Invalid,
            }
        }
    }
}

fn active_pointer_target_report(
    install_root: &Path,
    pointer: &Path,
    target: PathBuf,
    diagnostics: &mut Vec<InstallStateDiagnostic>,
) -> ActivePointerReport {
    if path_has_parent_component(&target) {
        diagnostics.push(diagnostic(
            InstallStateDiagnosticCode::ActivePointerTraversal,
            Some("active_pointer"),
            Some(target.clone()),
            format!("active pointer target contains '..': {}", target.display()),
        ));
        return ActivePointerReport {
            path: pointer.to_path_buf(),
            target: Some(target.clone()),
            version_dir: Some(target),
            release_tag: None,
            status: ActivePointerStatus::Invalid,
        };
    }
    if !target.is_absolute() {
        diagnostics.push(diagnostic(
            InstallStateDiagnosticCode::ActivePointerOutsideInstallRoot,
            Some("active_pointer"),
            Some(target.clone()),
            format!(
                "active pointer target must be absolute, got {}",
                target.display()
            ),
        ));
        return ActivePointerReport {
            path: pointer.to_path_buf(),
            target: Some(target.clone()),
            version_dir: Some(target),
            release_tag: None,
            status: ActivePointerStatus::Invalid,
        };
    }
    let resolved_target = target.clone();
    let invalid = active_target_diagnostic(install_root, &resolved_target);
    if let Some((code, message)) = invalid {
        diagnostics.push(diagnostic(
            code,
            Some("active_pointer"),
            Some(resolved_target.clone()),
            message,
        ));
        return ActivePointerReport {
            path: pointer.to_path_buf(),
            target: Some(target),
            version_dir: Some(resolved_target),
            release_tag: None,
            status: ActivePointerStatus::Invalid,
        };
    }
    let release_tag = release_tag_from_version_dir(&resolved_target);
    if !resolved_target.exists() {
        diagnostics.push(diagnostic(
            InstallStateDiagnosticCode::DanglingActivePointer,
            Some("active_pointer"),
            Some(resolved_target.clone()),
            "active pointer target does not exist".to_owned(),
        ));
        return ActivePointerReport {
            path: pointer.to_path_buf(),
            target: Some(target),
            version_dir: Some(resolved_target),
            release_tag,
            status: ActivePointerStatus::Dangling,
        };
    }
    match std::fs::symlink_metadata(&resolved_target) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            diagnostics.push(diagnostic(
                InstallStateDiagnosticCode::ActivePointerNotVersionDir,
                Some("active_pointer"),
                Some(resolved_target.clone()),
                format!(
                    "active pointer target must be a real version directory, got {}",
                    resolved_target.display()
                ),
            ));
            return ActivePointerReport {
                path: pointer.to_path_buf(),
                target: Some(target),
                version_dir: Some(resolved_target),
                release_tag,
                status: ActivePointerStatus::Invalid,
            };
        }
        Ok(_) => {}
        Err(source) => {
            diagnostics.push(diagnostic(
                InstallStateDiagnosticCode::ActivePointerUnreadable,
                Some("active_pointer"),
                Some(resolved_target.clone()),
                source.to_string(),
            ));
            return ActivePointerReport {
                path: pointer.to_path_buf(),
                target: Some(target),
                version_dir: Some(resolved_target),
                release_tag,
                status: ActivePointerStatus::Invalid,
            };
        }
    }
    ActivePointerReport {
        path: pointer.to_path_buf(),
        target: Some(target),
        version_dir: Some(resolved_target),
        release_tag,
        status: ActivePointerStatus::Live,
    }
}

fn active_target_diagnostic(
    install_root: &Path,
    target: &Path,
) -> Option<(InstallStateDiagnosticCode, String)> {
    if path_has_parent_component(target) {
        return Some((
            InstallStateDiagnosticCode::ActivePointerTraversal,
            format!("active pointer target contains '..': {}", target.display()),
        ));
    }
    let versions_dir = install_root.join(VERSIONS_DIR_NAME);
    if !target.starts_with(&versions_dir) {
        return Some((
            InstallStateDiagnosticCode::ActivePointerOutsideInstallRoot,
            format!(
                "active pointer target must be under {}, got {}",
                versions_dir.display(),
                target.display()
            ),
        ));
    }
    if target.parent() != Some(versions_dir.as_path()) {
        return Some((
            InstallStateDiagnosticCode::ActivePointerNotVersionDir,
            format!(
                "active pointer target must be one version directory under {}, got {}",
                versions_dir.display(),
                target.display()
            ),
        ));
    }
    None
}
