use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};

use nix::libc::O_NOFOLLOW;
use sha2::{Digest, Sha256};

const HASH_BUFFER_SIZE: usize = 64 * 1024;

pub(super) fn version_relative_path(
    version_dir: &Path,
    path: &Path,
    label: &str,
) -> Result<PathBuf, String> {
    let relative_path = path.strip_prefix(version_dir).map_err(|source| {
        format!(
            "{label} must stay under version dir {}: {}: {source}",
            version_dir.display(),
            path.display()
        )
    })?;
    let raw = relative_path.display().to_string();
    validate_relative_path(label, &raw)?;
    Ok(relative_path.to_path_buf())
}

pub(super) fn reject_symlink_ancestors(
    version_dir: &Path,
    relative_path: &Path,
    label: &str,
) -> Result<(), String> {
    let components = normal_components(relative_path, label)?;
    let Some((_, ancestors)) = components.split_last() else {
        return Err(format!("{label} must name a file below the version dir"));
    };
    reject_symlink_component_names(version_dir, ancestors, label)
}

pub(super) fn reject_symlink_components(
    version_dir: &Path,
    relative_path: &Path,
    label: &str,
) -> Result<(), String> {
    let components = normal_components(relative_path, label)?;
    reject_symlink_component_names(version_dir, &components, label)
}

fn normal_components<'a>(path: &'a Path, label: &str) -> Result<Vec<&'a std::ffi::OsStr>, String> {
    path.components()
        .map(|component| match component {
            Component::Normal(name) => Ok(name),
            Component::CurDir => Err(format!("{label} must not contain '.': {}", path.display())),
            Component::ParentDir => {
                Err(format!("{label} must not contain '..': {}", path.display()))
            }
            Component::RootDir | Component::Prefix(_) => {
                Err(format!("{label} must be relative: {}", path.display()))
            }
        })
        .collect()
}

fn reject_symlink_component_names(
    version_dir: &Path,
    components: &[&std::ffi::OsStr],
    label: &str,
) -> Result<(), String> {
    let mut cursor = version_dir.to_path_buf();
    for component in components {
        cursor.push(component);
        let metadata = match fs::symlink_metadata(&cursor) {
            Ok(metadata) => metadata,
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(source) => {
                return Err(format!(
                    "{label} path component is unreadable: path={} source={source}",
                    cursor.display()
                ));
            }
        };
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "{label} must not traverse symlink: path={}",
                cursor.display()
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_relative_path(label: &str, raw: &str) -> Result<(), String> {
    if raw.is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    if raw.contains('\\') {
        return Err(format!("{label} must not contain backslashes: {raw}"));
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        return Err(format!("{label} must be relative: {raw}"));
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(format!("{label} must not contain '..': {raw}"));
    }
    Ok(())
}

pub(super) fn validate_file_name(label: &str, raw: &str) -> Result<(), String> {
    validate_relative_path(label, raw)?;
    if raw == "." || raw.contains('/') {
        return Err(format!(
            "{label} must be a file name inside its directory: {raw}"
        ));
    }
    Ok(())
}

pub(super) fn require_nonempty(label: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        Err(format!("{label} must not be empty"))
    } else {
        Ok(())
    }
}

pub(super) fn require_sha256(label: &str, value: &str) -> Result<(), String> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(format!("{label} must be a 64-hex sha256 digest"))
    }
}

pub(super) fn read_file_nofollow(path: &Path) -> Result<Vec<u8>, io::Error> {
    let mut file = open_regular_file_nofollow(path)?;
    let mut raw = Vec::new();
    file.read_to_end(&mut raw)?;
    Ok(raw)
}

pub(super) fn hash_file_nofollow(path: &Path) -> Result<(String, u64), io::Error> {
    let mut file = open_regular_file_nofollow(path)?;
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    let mut buf = [0u8; HASH_BUFFER_SIZE];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hasher.update(&buf[..n]);
    }
    Ok((format!("{:x}", hasher.finalize()), size))
}

fn open_regular_file_nofollow(path: &Path) -> Result<File, io::Error> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("not a regular file: {}", path.display()),
        ));
    }
    Ok(file)
}

pub(super) fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
