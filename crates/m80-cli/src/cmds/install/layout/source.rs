use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use m80_firecracker::{ConfigError, FcError};

use super::bundle::sha256_file;

const STAGED_BUNDLE_NAME: &str = "bundle.tar.gz";
const STAGED_CHECKSUM_NAME: &str = "bundle.tar.gz.sha256";

pub(super) fn stage_bundle_source(
    bundle_url: &str,
    staging_dir: &Path,
) -> Result<PathBuf, FcError> {
    if let Some(path) = local_file_url_path(bundle_url)? {
        return stage_local_bundle(&path, staging_dir);
    }
    stage_remote_bundle(bundle_url, staging_dir)
}

pub(super) fn validate_bundle_source_url(bundle_url: &str) -> Result<(), FcError> {
    if local_file_url_path(bundle_url)?.is_some() {
        return Ok(());
    }
    parse_supported_initial_remote_url(bundle_url).map(|_| ())
}

pub(super) fn is_fixture_bundle_url(bundle_url: &str) -> Result<bool, FcError> {
    if local_file_url_path(bundle_url)?.is_some() {
        return Ok(true);
    }
    let parsed = parse_supported_initial_remote_url(bundle_url)?;
    Ok(parsed.scheme == "http" && is_local_fixture_host(&parsed.host))
}

fn local_file_url_path(url: &str) -> Result<Option<PathBuf>, FcError> {
    let Some(path) = url.strip_prefix("file://") else {
        return Ok(None);
    };
    let path = PathBuf::from(path);
    if path.is_absolute() {
        Ok(Some(path))
    } else {
        Err(FcError::Config(ConfigError::InvalidValue {
            field: "bundle-url",
            reason: "file:// bundle URL must contain an absolute local path".into(),
        }))
    }
}

fn stage_local_bundle(source: &Path, staging_dir: &Path) -> Result<PathBuf, FcError> {
    if !source.is_file() {
        return Err(FcError::ArtifactMissing {
            path: source.to_path_buf(),
        });
    }
    let staged = staging_dir.join(STAGED_BUNDLE_NAME);
    fs::copy(source, &staged).map_err(|source| FcError::PathIo {
        path: staged.clone(),
        source,
    })?;
    Ok(staged)
}

fn stage_remote_bundle(bundle_url: &str, staging_dir: &Path) -> Result<PathBuf, FcError> {
    let initial = parse_supported_initial_remote_url(bundle_url)?;
    let staged = staging_dir.join(STAGED_BUNDLE_NAME);
    let checksum = staging_dir.join(STAGED_CHECKSUM_NAME);
    let checksum_url = format!("{bundle_url}.sha256");

    let result = (|| {
        let final_url = download_url(bundle_url, &staged, "curl release bundle tarball")?;
        validate_final_url(&initial, &parse_supported_download_url(&final_url)?)?;
        let final_checksum_url =
            download_url(&checksum_url, &checksum, "curl release bundle checksum")?;
        validate_final_url(
            &initial,
            &parse_supported_download_url(&final_checksum_url)?,
        )?;
        verify_staged_bundle_checksum(&staged, &checksum)?;
        Ok(staged.clone())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&staged);
        let _ = fs::remove_file(&checksum);
    }
    result
}

fn download_url(url: &str, dest: &Path, command: &'static str) -> Result<String, FcError> {
    let output = Command::new("curl")
        .arg("-fsSL")
        .arg("--proto")
        .arg("=https,http")
        .arg("--proto-redir")
        .arg("=https,http")
        .arg("-o")
        .arg(dest)
        .arg("-w")
        .arg("%{url_effective}")
        .arg(url)
        .output()
        .map_err(|source| FcError::CommandSpawnFailed { command, source })?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(FcError::CommandFailed {
            command,
            status: output.status,
            output: command_output_text(&output),
        })
    }
}

fn verify_staged_bundle_checksum(bundle: &Path, checksum: &Path) -> Result<(), FcError> {
    let expected = read_expected_sha256(checksum)?;
    let actual = sha256_file(bundle)?;
    if actual == expected {
        Ok(())
    } else {
        Err(FcError::Config(ConfigError::InvalidValue {
            field: "bundle-url.sha256",
            reason: format!("bundle sha256 mismatch: expected {expected}, got {actual}"),
        }))
    }
}

fn read_expected_sha256(checksum_file: &Path) -> Result<String, FcError> {
    let text = fs::read_to_string(checksum_file).map_err(|source| FcError::PathIo {
        path: checksum_file.to_path_buf(),
        source,
    })?;
    let expected = text.split_whitespace().next().ok_or_else(|| {
        FcError::Config(ConfigError::InvalidValue {
            field: "bundle-url.sha256",
            reason: format!("checksum file {} is empty", checksum_file.display()),
        })
    })?;
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "bundle-url.sha256",
            reason: format!(
                "checksum file {} must start with a 64-hex sha256 digest",
                checksum_file.display()
            ),
        }));
    }
    Ok(expected.to_ascii_lowercase())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteUrl {
    scheme: String,
    authority: String,
    host: String,
    path: String,
}

fn parse_supported_initial_remote_url(url: &str) -> Result<RemoteUrl, FcError> {
    parse_supported_remote_url(url, false)
}

fn parse_supported_download_url(url: &str) -> Result<RemoteUrl, FcError> {
    parse_supported_remote_url(url, true)
}

fn parse_supported_remote_url(
    url: &str,
    allow_github_asset_redirect_host: bool,
) -> Result<RemoteUrl, FcError> {
    let parsed = parse_url(url)?;
    match parsed.scheme.as_str() {
        "https" if is_github_release_url(&parsed) => Ok(parsed),
        "http" if is_local_fixture_host(&parsed.host) => Ok(parsed),
        "https"
            if allow_github_asset_redirect_host
                && is_github_asset_redirect_host(&parsed.host) =>
        {
            Ok(parsed)
        }
        "https" | "http" => Err(FcError::Config(ConfigError::InvalidValue {
            field: "bundle-url",
            reason: format!(
                "remote bundle URL must be a moradology/m80 GitHub release asset or local test fixture: {url}"
            ),
        })),
        _ => Err(FcError::UnsupportedOperation {
            operation: "m80 install",
            reason: "bundle URL must use file://, https:// release assets, or local http:// test fixtures".into(),
        }),
    }
}

fn parse_url(url: &str) -> Result<RemoteUrl, FcError> {
    let (scheme, rest) = url.split_once("://").ok_or_else(|| {
        FcError::UnsupportedOperation {
            operation: "m80 install",
            reason: "bundle URL must use file://, https:// release assets, or local http:// test fixtures".into(),
        }
    })?;
    let scheme = scheme.to_ascii_lowercase();
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, format!("/{path}")),
        None => (rest, "/".to_owned()),
    };
    if authority.is_empty() || authority.contains('@') {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "bundle-url",
            reason: format!("remote bundle URL has unsupported authority: {url}"),
        }));
    }
    let host = parse_authority_host(authority, url)?;
    if host.is_empty() {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "bundle-url",
            reason: format!("remote bundle URL has empty host: {url}"),
        }));
    }
    Ok(RemoteUrl {
        scheme,
        authority: authority.to_ascii_lowercase(),
        host,
        path,
    })
}

fn parse_authority_host(authority: &str, url: &str) -> Result<String, FcError> {
    if let Some(rest) = authority.strip_prefix('[') {
        let Some((host, suffix)) = rest.split_once(']') else {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "bundle-url",
                reason: format!("remote bundle URL has malformed bracketed host: {url}"),
            }));
        };
        if !suffix.is_empty() && !suffix.starts_with(':') {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "bundle-url",
                reason: format!("remote bundle URL has unsupported authority: {url}"),
            }));
        }
        return Ok(host.to_ascii_lowercase());
    }
    if authority.contains('[') || authority.contains(']') {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "bundle-url",
            reason: format!("remote bundle URL has malformed bracketed host: {url}"),
        }));
    }
    Ok(authority
        .rsplit_once(':')
        .map(|(host, _)| host)
        .unwrap_or(authority)
        .to_ascii_lowercase())
}

fn validate_final_url(initial: &RemoteUrl, final_url: &RemoteUrl) -> Result<(), FcError> {
    if is_local_fixture_host(&initial.host) {
        if initial.authority == final_url.authority {
            return Ok(());
        }
    } else if is_github_release_url(initial)
        && (is_github_release_url(final_url) || is_github_asset_redirect_host(&final_url.host))
    {
        return Ok(());
    }
    Err(FcError::Config(ConfigError::InvalidValue {
        field: "bundle-url",
        reason: format!(
            "release bundle download redirected to unsupported host {}; expected {}",
            final_url.authority, initial.authority
        ),
    }))
}

fn is_github_release_url(url: &RemoteUrl) -> bool {
    url.scheme == "https"
        && url.host == "github.com"
        && url.path.starts_with("/moradology/m80/releases/download/")
}

fn is_github_asset_redirect_host(host: &str) -> bool {
    matches!(
        host,
        "objects.githubusercontent.com"
            | "github-releases.githubusercontent.com"
            | "release-assets.githubusercontent.com"
    )
}

fn is_local_fixture_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
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
    fn remote_url_rejects_non_release_https_host() {
        let err =
            parse_supported_initial_remote_url("https://example.invalid/m80-linux-x86_64.tar.gz")
                .unwrap_err();
        assert!(
            err.to_string()
                .contains("moradology/m80 GitHub release asset"),
            "{err}"
        );
    }

    #[test]
    fn initial_remote_url_rejects_github_cdn_host() {
        let err = parse_supported_initial_remote_url(
            "https://release-assets.githubusercontent.com/github-production-release-asset/file",
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("moradology/m80 GitHub release asset"),
            "{err}"
        );
    }

    #[test]
    fn local_fixture_redirect_must_stay_on_same_authority() {
        let initial =
            parse_supported_initial_remote_url("http://127.0.0.1:1234/bundle.tar.gz").unwrap();
        let final_url =
            parse_supported_download_url("http://localhost:1234/bundle.tar.gz").unwrap();

        let err = validate_final_url(&initial, &final_url).unwrap_err();
        assert!(
            err.to_string().contains("redirected to unsupported host"),
            "{err}"
        );
    }

    #[test]
    fn local_fixture_ipv6_host_without_port_is_supported() {
        let parsed =
            parse_supported_initial_remote_url("http://[::1]/m80-linux-x86_64.tar.gz").unwrap();

        assert_eq!(parsed.host, "::1");
        assert_eq!(parsed.authority, "[::1]");
    }
}
