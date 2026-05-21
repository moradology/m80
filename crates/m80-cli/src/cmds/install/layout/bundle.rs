use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use m80_firecracker::{ConfigError, FcError};
use sha2::{Digest, Sha256};

pub(super) const REQUIRED_BUNDLE_FILES: &[&str] = &[
    "bin/m80",
    "bin/m80-jailer-harden",
    "bin/m80-net-helper",
    "artifacts/vmlinux",
    "artifacts/output.ext4",
    "artifacts/output.ext4.manifest.json",
    "artifacts/output.ext4.build-receipt.json",
    "artifacts/m80-guestd",
    "install.sh",
    "bundle.json",
    "SHA256SUMS",
];
pub(super) const PAYLOAD_FILES: &[&str] = &[
    "bin/m80",
    "bin/m80-jailer-harden",
    "bin/m80-net-helper",
    "artifacts/vmlinux",
    "artifacts/output.ext4",
    "artifacts/output.ext4.manifest.json",
    "artifacts/output.ext4.build-receipt.json",
    "artifacts/m80-guestd",
    "install.sh",
];

pub(super) fn list_bundle_entries(bundle_path: &Path) -> Result<Vec<String>, FcError> {
    let output = Command::new("tar")
        .arg("-tzf")
        .arg(bundle_path)
        .output()
        .map_err(|source| FcError::CommandSpawnFailed {
            command: "list release bundle",
            source,
        })?;
    if !output.status.success() {
        return Err(FcError::CommandFailed {
            command: "list release bundle",
            status: output.status,
            output: command_output_text(&output),
        });
    }
    let mut entries = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(entry) = normalize_tar_entry(line)? {
            entries.push(entry);
        }
    }
    Ok(entries)
}

pub(super) fn verify_entry_set(entries: &[String]) -> Result<(), FcError> {
    let required = REQUIRED_BUNDLE_FILES
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    for entry in entries {
        if !seen.insert(entry.as_str()) {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "bundle",
                reason: format!("bundle duplicate path: {entry}"),
            }));
        }
        if !required.contains(entry.as_str()) {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "bundle",
                reason: format!("bundle unexpected path: {entry}"),
            }));
        }
    }
    for required in REQUIRED_BUNDLE_FILES {
        if !seen.contains(required) {
            return Err(FcError::ArtifactMissing {
                path: PathBuf::from(required),
            });
        }
    }
    Ok(())
}

fn normalize_tar_entry(entry: &str) -> Result<Option<String>, FcError> {
    let mut trimmed = entry;
    while let Some(rest) = trimmed.strip_prefix("./") {
        trimmed = rest;
    }
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.contains('\\') {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "bundle",
            reason: format!("bundle path must use '/' separators: {trimmed}"),
        }));
    }
    let is_dir = trimmed.ends_with('/');
    let trimmed_path = trimmed.trim_end_matches('/');
    if trimmed_path.is_empty() {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "bundle",
            reason: format!("bundle path must stay inside the install layout: {trimmed}"),
        }));
    }
    let path = Path::new(trimmed_path);
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            _ => {
                return Err(FcError::Config(ConfigError::InvalidValue {
                    field: "bundle",
                    reason: format!(
                        "bundle path must stay inside the install layout: {trimmed_path}"
                    ),
                }));
            }
        }
    }
    if is_dir || parts.is_empty() {
        Ok(None)
    } else {
        Ok(Some(parts.join("/")))
    }
}

pub(super) fn verify_extracted_tree(root: &Path) -> Result<(), FcError> {
    let required = REQUIRED_BUNDLE_FILES
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let allowed_dirs = ["bin", "artifacts"].into_iter().collect::<BTreeSet<_>>();
    verify_extracted_tree_inner(root, root, &required, &allowed_dirs)
}

fn verify_extracted_tree_inner(
    root: &Path,
    dir: &Path,
    required: &BTreeSet<&str>,
    allowed_dirs: &BTreeSet<&str>,
) -> Result<(), FcError> {
    for entry in fs::read_dir(dir).map_err(|source| FcError::PathIo {
        path: dir.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| FcError::PathIo {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let file_type = fs::symlink_metadata(&path)
            .map_err(|source| FcError::PathIo {
                path: path.clone(),
                source,
            })?
            .file_type();
        if file_type.is_dir() {
            if path != root {
                let rel = path
                    .strip_prefix(root)
                    .expect("walked path remains under extraction root")
                    .to_string_lossy()
                    .replace('\\', "/");
                if !allowed_dirs.contains(rel.as_str()) {
                    return Err(FcError::Config(ConfigError::InvalidValue {
                        field: "bundle",
                        reason: format!("bundle unexpected extracted directory: {rel}"),
                    }));
                }
            }
            verify_extracted_tree_inner(root, &path, required, allowed_dirs)?;
        } else if file_type.is_file() {
            let rel = path
                .strip_prefix(root)
                .expect("walked path remains under extraction root")
                .to_string_lossy()
                .replace('\\', "/");
            if !required.contains(rel.as_str()) {
                return Err(FcError::Config(ConfigError::InvalidValue {
                    field: "bundle",
                    reason: format!("bundle unexpected extracted path: {rel}"),
                }));
            }
        } else {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "bundle",
                reason: format!("bundle contains non-regular file: {}", path.display()),
            }));
        }
    }
    Ok(())
}

pub(in crate::cmds::install::layout) fn extract_bundle(
    bundle_path: &Path,
    dest: &Path,
) -> Result<(), FcError> {
    let output = Command::new("tar")
        .arg("-xzf")
        .arg(bundle_path)
        .arg("-C")
        .arg(dest)
        .output()
        .map_err(|source| FcError::CommandSpawnFailed {
            command: "extract release bundle",
            source,
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(FcError::CommandFailed {
            command: "extract release bundle",
            status: output.status,
            output: command_output_text(&output),
        })
    }
}

pub(super) fn sha256_file(path: &Path) -> Result<String, FcError> {
    let bytes = fs::read(path).map_err(|source| FcError::PathIo {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

fn command_output_text(output: &std::process::Output) -> String {
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if combined.is_empty() {
        String::new()
    } else {
        format!(": {combined}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_tar_entry_keeps_bundle_paths_and_ignores_dirs() {
        assert_eq!(
            normalize_tar_entry("./bin/m80").unwrap().as_deref(),
            Some("bin/m80")
        );
        assert_eq!(normalize_tar_entry("./artifacts/").unwrap(), None);
    }

    #[test]
    fn normalize_tar_entry_rejects_escaping_directories_before_extract() {
        let err = normalize_tar_entry("../outside/").unwrap_err();
        assert!(
            err.to_string().contains("stay inside the install layout"),
            "{err}"
        );
    }

    #[test]
    fn verify_entry_set_rejects_unexpected_files() {
        let mut entries = REQUIRED_BUNDLE_FILES
            .iter()
            .map(|path| (*path).to_owned())
            .collect::<Vec<_>>();
        entries.push("README.md".to_owned());

        let err = verify_entry_set(&entries).unwrap_err();
        assert!(err.to_string().contains("unexpected path"), "{err}");
    }
}
