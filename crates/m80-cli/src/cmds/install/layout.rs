use std::fs;
use std::path::{Path, PathBuf};

use m80_firecracker::{ConfigError, FcError};
use serde::Serialize;

use super::{InstallPlan, SourceKind};
use bundle::{
    extract_bundle, list_bundle_entries, verify_entry_set, verify_extracted_tree,
    REQUIRED_BUNDLE_FILES,
};
use metadata::{
    read_bundle_metadata, rewrite_installed_metadata, set_final_modes, verify_bundle_metadata,
    verify_metadata_hashes, verify_sha256s_file, INSTALL_PROVENANCE_FILE,
};

mod bundle;
mod metadata;

/// Summary emitted after the layout copy succeeds.
#[derive(Debug, Serialize)]
pub(super) struct LayoutInstallSummary {
    pub(super) release_tag: String,
    pub(super) version_dir: String,
    pub(super) files_copied: usize,
    pub(super) install_provenance: String,
    pub(super) active_pointer_unchanged: bool,
    pub(super) profile_written: bool,
}

pub(super) fn install_bundle_layout(plan: &InstallPlan) -> Result<LayoutInstallSummary, FcError> {
    let bundle_url = require_explicit_bundle_url(plan)?;
    let bundle_path = local_file_url_path(bundle_url)?;
    if !bundle_path.is_file() {
        return Err(FcError::ArtifactMissing { path: bundle_path });
    }

    let install_root = PathBuf::from(&plan.install_root);
    require_absolute_path("install_root", &install_root)?;
    let entries = list_bundle_entries(&bundle_path)?;
    verify_entry_set(&entries)?;

    let staging_dir = prepare_staging_dir(&install_root)?;
    let extracted_dir = staging_dir.join("bundle");
    fs::create_dir(&extracted_dir).map_err(|source| FcError::PathIo {
        path: extracted_dir.clone(),
        source,
    })?;
    extract_bundle(&bundle_path, &extracted_dir)?;
    verify_extracted_tree(&extracted_dir)?;

    let metadata = read_bundle_metadata(&extracted_dir.join("bundle.json"))?;
    verify_bundle_metadata(&metadata)?;
    if let Some(source_tag) = plan.source.release_tag.as_deref() {
        if source_tag != metadata.release_tag {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "bundle.release_tag",
                reason: format!(
                    "bundle release_tag mismatch: source selected {source_tag}, bundle contains {}",
                    metadata.release_tag
                ),
            }));
        }
    }
    verify_metadata_hashes(&extracted_dir, &metadata)?;
    verify_sha256s_file(&extracted_dir)?;

    let final_dir = install_root
        .join("versions")
        .join(safe_release_dir(&metadata.release_tag)?);
    if final_dir.exists() {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "install.version_dir",
            reason: format!("version directory already exists: {}", final_dir.display()),
        }));
    }

    rewrite_installed_metadata(&extracted_dir, &final_dir, &metadata)?;
    set_final_modes(&extracted_dir)?;
    let versions_dir = final_dir
        .parent()
        .expect("version directory should have versions parent");
    fs::create_dir_all(versions_dir).map_err(|source| FcError::PathIo {
        path: versions_dir.to_path_buf(),
        source,
    })?;
    fs::rename(&extracted_dir, &final_dir).map_err(|source| FcError::PathIo {
        path: final_dir.clone(),
        source,
    })?;
    let _ = fs::remove_dir(&staging_dir);

    let install_provenance = final_dir.join("artifacts").join(INSTALL_PROVENANCE_FILE);
    Ok(LayoutInstallSummary {
        release_tag: metadata.release_tag,
        version_dir: final_dir.display().to_string(),
        files_copied: REQUIRED_BUNDLE_FILES.len() + 1,
        install_provenance: install_provenance.display().to_string(),
        active_pointer_unchanged: true,
        profile_written: false,
    })
}

fn require_explicit_bundle_url(plan: &InstallPlan) -> Result<&str, FcError> {
    if plan.source.kind != SourceKind::BundleUrl {
        return Err(FcError::UnsupportedOperation {
            operation: "m80 install",
            reason: "layout copy currently requires --bundle-url; release-tag resolution lands in the asset-index/bootstrapper leaves".into(),
        });
    }
    plan.source
        .bundle_url
        .as_deref()
        .ok_or(FcError::Config(ConfigError::MissingField {
            field: "bundle-url",
        }))
}

fn local_file_url_path(url: &str) -> Result<PathBuf, FcError> {
    let Some(path) = url.strip_prefix("file://") else {
        return Err(FcError::UnsupportedOperation {
            operation: "m80 install",
            reason: "layout copy currently supports local file:// bundle URLs; network download lands in the bootstrapper/asset-index leaves".into(),
        });
    };
    let path = PathBuf::from(path);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(FcError::Config(ConfigError::InvalidValue {
            field: "bundle-url",
            reason: "file:// bundle URL must contain an absolute local path".into(),
        }))
    }
}

fn require_absolute_path(field: &'static str, path: &Path) -> Result<(), FcError> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(FcError::Config(ConfigError::InvalidValue {
            field,
            reason: format!("{field} must be an absolute path, got {}", path.display()),
        }))
    }
}

fn prepare_staging_dir(install_root: &Path) -> Result<PathBuf, FcError> {
    let staging_parent = install_root.join(".staging");
    fs::create_dir_all(&staging_parent).map_err(|source| FcError::PathIo {
        path: staging_parent.clone(),
        source,
    })?;
    let staging_dir = staging_parent.join(format!("layout-{}", std::process::id()));
    if staging_dir.exists() {
        fs::remove_dir_all(&staging_dir).map_err(|source| FcError::PathIo {
            path: staging_dir.clone(),
            source,
        })?;
    }
    fs::create_dir(&staging_dir).map_err(|source| FcError::PathIo {
        path: staging_dir.clone(),
        source,
    })?;
    Ok(staging_dir)
}

fn safe_release_dir(release_tag: &str) -> Result<&str, FcError> {
    if release_tag.is_empty()
        || release_tag.contains('/')
        || release_tag.contains('\\')
        || release_tag == "."
        || release_tag == ".."
    {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "bundle.release_tag",
            reason: format!("release tag is not a safe directory name: {release_tag:?}"),
        }));
    }
    Ok(release_tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_file_url_requires_absolute_path() {
        let err = local_file_url_path("file://relative.tar.gz").unwrap_err();
        assert!(
            err.to_string().contains("absolute local path"),
            "unexpected error: {err}"
        );
    }
}
