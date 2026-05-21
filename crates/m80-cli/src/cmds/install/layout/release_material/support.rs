use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use m80_firecracker::{ConfigError, FcError};
use sha2::{Digest, Sha256};

use super::ReleaseMaterial;

pub(super) fn download_material_to_dir(
    material: &ReleaseMaterial,
    dir: &Path,
) -> Result<PathBuf, FcError> {
    let dest = dir.join(&material.name);
    download_material_to_path(material, &dest)
}

fn download_material_to_path(material: &ReleaseMaterial, dest: &Path) -> Result<PathBuf, FcError> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .map_err(|source| FcError::PathIo {
            path: dest.to_path_buf(),
            source,
        })?;
    let output = Command::new("curl")
        .arg("-fsSL")
        .arg("--connect-timeout")
        .arg(super::CONNECT_TIMEOUT_SECONDS)
        .arg("--max-time")
        .arg(super::MAX_TIME_SECONDS)
        .arg("--proto")
        .arg("=https,http")
        .arg("--proto-redir")
        .arg("=https,http")
        .arg("-o")
        .arg(dest)
        .arg("-w")
        .arg("%{url_effective}")
        .arg(&material.url)
        .output()
        .map_err(|source| {
            let _ = fs::remove_file(dest);
            release_material_error(format!(
                "release material fetch spawn failed: {} source={source}",
                material.context()
            ))
        })?;
    let final_url = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !output.status.success() {
        let _ = fs::remove_file(dest);
        return Err(release_material_error(format!(
            "release material fetch failed: {} status={}{}",
            material.context(),
            output.status,
            command_stderr_text(&output)
        )));
    }
    if let Err(err) = validate_final_material_url(material, &final_url) {
        let _ = fs::remove_file(dest);
        return Err(err);
    }
    Ok(dest.to_path_buf())
}

pub(super) fn read_checksum_line(path: &Path) -> Result<(String, Option<String>), FcError> {
    let text = fs::read_to_string(path).map_err(|source| FcError::PathIo {
        path: path.to_path_buf(),
        source,
    })?;
    let mut parts = text.split_whitespace();
    let Some(sha256) = parts.next() else {
        return Err(release_material_error(format!(
            "release material checksum is empty: path={}",
            path.display()
        )));
    };
    if !is_sha256(sha256) {
        return Err(release_material_error(format!(
            "release material checksum must start with a 64-hex sha256 digest: path={}",
            path.display()
        )));
    }
    Ok((sha256.to_ascii_lowercase(), parts.next().map(str::to_owned)))
}

pub(super) fn sha256_file(path: &Path) -> Result<String, FcError> {
    let bytes = fs::read(path).map_err(|source| FcError::PathIo {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

pub(super) fn release_asset_url(release_tag: &str, name: &str) -> String {
    crate::release_urls::release_asset_url(release_tag, name)
}

pub(super) fn validate_release_asset_name(name: &str) -> Result<(), FcError> {
    if !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Ok(());
    }
    Err(release_material_error(format!(
        "release material asset name is invalid: material_name={name:?}"
    )))
}

pub(super) fn release_material_error(reason: String) -> FcError {
    FcError::Config(ConfigError::InvalidValue {
        field: "release-material",
        reason,
    })
}

fn validate_final_material_url(material: &ReleaseMaterial, final_url: &str) -> Result<(), FcError> {
    if final_url == material.url {
        return Ok(());
    }
    if https_host(final_url)
        .as_deref()
        .is_some_and(is_github_asset_redirect_host)
    {
        return Ok(());
    }
    Err(release_material_error(format!(
        "release material redirected to unsupported URL: {} final_url={final_url}",
        material.context()
    )))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn https_host(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let authority = rest.split('/').next()?;
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let host = authority
        .rsplit_once(':')
        .map(|(host, _)| host)
        .unwrap_or(authority);
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

fn is_github_asset_redirect_host(host: &str) -> bool {
    matches!(
        host,
        "objects.githubusercontent.com"
            | "github-releases.githubusercontent.com"
            | "release-assets.githubusercontent.com"
    )
}

fn command_stderr_text(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let trimmed = stderr.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!(" stderr={}", trimmed.replace('\n', "\\n"))
    }
}
