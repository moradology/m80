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
    let expected = expected_material_identity(material)?;
    let requested = release_asset_identity(&material.url).ok_or_else(|| {
        release_redirect_identity_error(
            material,
            final_url,
            "requested_url",
            format!(
                "requested URL is not an official release asset: requested_url={}",
                material.url
            ),
        )
    })?;
    if requested != expected {
        return Err(release_redirect_identity_error(
            material,
            final_url,
            "requested_identity",
            format!(
                "requested URL does not match material identity: requested_repository={} requested_release_tag={} requested_asset_name={}",
                requested.repository, requested.release_tag, requested.asset_name
            ),
        ));
    }

    if final_url == material.url {
        return Ok(());
    }

    if let Some(final_identity) = release_asset_identity(final_url) {
        if final_identity == expected {
            return Ok(());
        }
        let rejected = final_identity_mismatch_field(&expected, &final_identity);
        return Err(release_redirect_identity_error(
            material,
            final_url,
            rejected,
            format!(
                "final URL does not match material identity: final_repository={} final_release_tag={} final_asset_name={}",
                final_identity.repository, final_identity.release_tag, final_identity.asset_name
            ),
        ));
    }

    let Some(final_host) = https_host(final_url) else {
        return Err(release_redirect_identity_error(
            material,
            final_url,
            "final_url",
            "final URL is not an HTTPS URL with a supported authority".to_owned(),
        ));
    };
    if is_github_asset_redirect_host(&final_host) {
        return Ok(());
    }
    Err(release_redirect_identity_error(
        material,
        final_url,
        "host",
        format!(
            "final URL host is not an official GitHub asset redirect host: final_host={final_host}"
        ),
    ))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Debug, PartialEq, Eq)]
struct ReleaseAssetIdentity {
    repository: String,
    release_tag: String,
    asset_name: String,
}

fn expected_material_identity(material: &ReleaseMaterial) -> Result<ReleaseAssetIdentity, FcError> {
    let Some(release_tag) = super::release_tag_from_material_url(&material.url) else {
        return Err(release_redirect_identity_error(
            material,
            &material.url,
            "release_tag",
            "material URL does not contain an official release tag".to_owned(),
        ));
    };
    Ok(ReleaseAssetIdentity {
        repository: crate::release_urls::release_repository(),
        release_tag: release_tag.to_owned(),
        asset_name: material.name.clone(),
    })
}

fn release_asset_identity(url: &str) -> Option<ReleaseAssetIdentity> {
    let rest = url.strip_prefix("https://github.com/")?;
    let (owner, rest) = rest.split_once('/')?;
    let (repo, rest) = rest.split_once('/')?;
    let rest = rest.strip_prefix("releases/download/")?;
    let (release_tag, asset_name) = rest.split_once('/')?;
    if owner.is_empty()
        || repo.is_empty()
        || release_tag.is_empty()
        || asset_name.is_empty()
        || asset_name.contains('/')
    {
        return None;
    }
    Some(ReleaseAssetIdentity {
        repository: format!("{owner}/{repo}"),
        release_tag: release_tag.to_owned(),
        asset_name: asset_name.to_owned(),
    })
}

fn final_identity_mismatch_field(
    expected: &ReleaseAssetIdentity,
    observed: &ReleaseAssetIdentity,
) -> &'static str {
    if observed.repository != expected.repository {
        "repository"
    } else if observed.release_tag != expected.release_tag {
        "release_tag"
    } else {
        "asset_name"
    }
}

fn release_redirect_identity_error(
    material: &ReleaseMaterial,
    final_url: &str,
    rejected_identity_field: &'static str,
    reason: String,
) -> FcError {
    release_material_error(format!(
        "release material redirect identity mismatch: material_role={} requested_url={} final_url={} expected_asset_name={} rejected_identity_field={} {}; {}",
        material.class,
        material.url,
        final_url,
        material.name,
        rejected_identity_field,
        reason,
        material.context()
    ))
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
