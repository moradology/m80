use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

use super::{HostTuple, ReleaseAssetIndex, ASSET_INDEX_NAME, DEFAULT_IMAGE_KIND};

static DOWNLOAD_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct AssetIndexFetchRequest<'a> {
    pub(super) index_url: &'a str,
    pub(super) release_tag: &'a str,
    pub(super) host: HostTuple<'a>,
    pub(super) image_kind: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct VerifiedAssetIndex {
    pub(super) index: ReleaseAssetIndex,
    pub(super) index_url: String,
    pub(super) checksum_url: String,
    pub(super) expected_sha256: String,
    pub(super) observed_sha256: String,
}

pub(super) fn github_release_asset_index_url(release_tag: &str) -> String {
    crate::release_urls::release_asset_url(release_tag, ASSET_INDEX_NAME)
}

pub(super) fn fetch_verified_asset_index(
    request: AssetIndexFetchRequest<'_>,
) -> Result<VerifiedAssetIndex, AssetIndexFetchError> {
    let context = AssetIndexFetchContext::new(request);
    let checksum_url = checksum_url_for_index(request.index_url);
    let index_bytes = fetch_asset_index_bytes(request.index_url, request.release_tag, &context)?;
    let checksum_bytes = fetch_asset_index_bytes(&checksum_url, request.release_tag, &context)?;
    let expected_sha256 = read_expected_index_sha256(&checksum_url, &checksum_bytes, &context)?;
    let observed_sha256 = sha256_bytes(&index_bytes);
    if expected_sha256 != observed_sha256 {
        return Err(AssetIndexFetchError::ChecksumMismatch {
            index_url: request.index_url.to_owned(),
            checksum_url,
            expected_sha256,
            observed_sha256,
            context,
        });
    }

    let index_text = String::from_utf8(index_bytes).map_err(|source| {
        AssetIndexFetchError::VerifiedIndexInvalid {
            index_url: request.index_url.to_owned(),
            checksum_url: checksum_url.clone(),
            expected_sha256: expected_sha256.clone(),
            observed_sha256: observed_sha256.clone(),
            detail: format!("release asset index is not UTF-8: {source}"),
            context: context.clone(),
        }
    })?;
    let index = ReleaseAssetIndex::parse_json(&index_text).map_err(|source| {
        AssetIndexFetchError::VerifiedIndexInvalid {
            index_url: request.index_url.to_owned(),
            checksum_url: checksum_url.clone(),
            expected_sha256: expected_sha256.clone(),
            observed_sha256: observed_sha256.clone(),
            detail: source.to_string(),
            context: context.clone(),
        }
    })?;
    if index.release_tag != request.release_tag {
        return Err(AssetIndexFetchError::ReleaseTagMismatch {
            index_url: request.index_url.to_owned(),
            checksum_url,
            expected_sha256,
            observed_sha256,
            expected_release_tag: request.release_tag.to_owned(),
            actual_release_tag: index.release_tag,
            context,
        });
    }
    Ok(VerifiedAssetIndex {
        index,
        index_url: request.index_url.to_owned(),
        checksum_url,
        expected_sha256,
        observed_sha256,
    })
}

fn checksum_url_for_index(index_url: &str) -> String {
    format!("{index_url}.sha256")
}

fn fetch_asset_index_bytes(
    url: &str,
    release_tag: &str,
    context: &AssetIndexFetchContext,
) -> Result<Vec<u8>, AssetIndexFetchError> {
    if let Some(path) = local_file_url_path(url, context)? {
        return fs::read(&path).map_err(|source| AssetIndexFetchError::LocalRead {
            path,
            source: source.to_string(),
            context: context.clone(),
        });
    }
    let initial = parse_supported_remote_index_url(url, release_tag, context)?;
    let (bytes, final_url) = download_url_to_bytes(url, context)?;
    let final_url = parse_supported_download_index_url(&final_url, release_tag, context)?;
    validate_final_index_url(&initial, &final_url, context)?;
    Ok(bytes)
}

fn local_file_url_path(
    url: &str,
    context: &AssetIndexFetchContext,
) -> Result<Option<PathBuf>, AssetIndexFetchError> {
    let Some(path) = url.strip_prefix("file://") else {
        return Ok(None);
    };
    let path = PathBuf::from(path);
    if path.is_absolute() {
        Ok(Some(path))
    } else {
        Err(AssetIndexFetchError::UnsupportedUrl {
            url: url.to_owned(),
            reason: "file:// release asset index URL must contain an absolute local path"
                .to_owned(),
            context: context.clone(),
        })
    }
}

fn download_url_to_bytes(
    url: &str,
    context: &AssetIndexFetchContext,
) -> Result<(Vec<u8>, String), AssetIndexFetchError> {
    let dest = temp_download_path();
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&dest)
        .map_err(|source| AssetIndexFetchError::LocalRead {
            path: dest.clone(),
            source: source.to_string(),
            context: context.clone(),
        })?;
    let output = Command::new("curl")
        .arg("-fsSL")
        .arg("--proto")
        .arg("=https,http")
        .arg("--proto-redir")
        .arg("=https,http")
        .arg("-o")
        .arg(&dest)
        .arg("-w")
        .arg("%{url_effective}")
        .arg(url)
        .output()
        .map_err(|source| AssetIndexFetchError::DownloadSpawnFailed {
            url: url.to_owned(),
            source: source.to_string(),
            context: context.clone(),
        })?;
    let final_url = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !output.status.success() {
        let _ = fs::remove_file(&dest);
        return Err(AssetIndexFetchError::DownloadFailed {
            url: url.to_owned(),
            status: output.status.to_string(),
            output: command_stderr_text(&output),
            context: context.clone(),
        });
    }
    let bytes = fs::read(&dest).map_err(|source| AssetIndexFetchError::LocalRead {
        path: dest.clone(),
        source: source.to_string(),
        context: context.clone(),
    })?;
    let _ = fs::remove_file(&dest);
    Ok((bytes, final_url))
}

fn temp_download_path() -> PathBuf {
    let count = DOWNLOAD_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "m80-release-index-{}-{count}.tmp",
        std::process::id()
    ))
}

fn read_expected_index_sha256(
    checksum_url: &str,
    checksum_bytes: &[u8],
    context: &AssetIndexFetchContext,
) -> Result<String, AssetIndexFetchError> {
    let text = std::str::from_utf8(checksum_bytes).map_err(|source| {
        AssetIndexFetchError::ChecksumInvalid {
            checksum_url: checksum_url.to_owned(),
            detail: format!("checksum sidecar is not UTF-8: {source}"),
            context: context.clone(),
        }
    })?;
    let expected =
        text.split_whitespace()
            .next()
            .ok_or_else(|| AssetIndexFetchError::ChecksumInvalid {
                checksum_url: checksum_url.to_owned(),
                detail: "checksum sidecar is empty".to_owned(),
                context: context.clone(),
            })?;
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AssetIndexFetchError::ChecksumInvalid {
            checksum_url: checksum_url.to_owned(),
            detail: "checksum sidecar must start with a 64-hex sha256 digest".to_owned(),
            context: context.clone(),
        });
    }
    Ok(expected.to_ascii_lowercase())
}

pub(super) fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AssetIndexFetchContext {
    release_tag: String,
    os: String,
    arch: String,
    image_kind: String,
}

impl AssetIndexFetchContext {
    fn new(request: AssetIndexFetchRequest<'_>) -> Self {
        Self {
            release_tag: request.release_tag.to_owned(),
            os: request.host.os.to_owned(),
            arch: request.host.arch.to_owned(),
            image_kind: request.image_kind.unwrap_or(DEFAULT_IMAGE_KIND).to_owned(),
        }
    }

    pub(super) fn describe(&self) -> String {
        format!(
            "release_tag={} host={}/{} image_kind={}",
            self.release_tag, self.os, self.arch, self.image_kind
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum AssetIndexFetchError {
    UnsupportedUrl {
        url: String,
        reason: String,
        context: AssetIndexFetchContext,
    },
    LocalRead {
        path: PathBuf,
        source: String,
        context: AssetIndexFetchContext,
    },
    DownloadSpawnFailed {
        url: String,
        source: String,
        context: AssetIndexFetchContext,
    },
    DownloadFailed {
        url: String,
        status: String,
        output: String,
        context: AssetIndexFetchContext,
    },
    RedirectUnsupported {
        initial: String,
        final_url: String,
        context: AssetIndexFetchContext,
    },
    ChecksumInvalid {
        checksum_url: String,
        detail: String,
        context: AssetIndexFetchContext,
    },
    ChecksumMismatch {
        index_url: String,
        checksum_url: String,
        expected_sha256: String,
        observed_sha256: String,
        context: AssetIndexFetchContext,
    },
    VerifiedIndexInvalid {
        index_url: String,
        checksum_url: String,
        expected_sha256: String,
        observed_sha256: String,
        detail: String,
        context: AssetIndexFetchContext,
    },
    ReleaseTagMismatch {
        index_url: String,
        checksum_url: String,
        expected_sha256: String,
        observed_sha256: String,
        expected_release_tag: String,
        actual_release_tag: String,
        context: AssetIndexFetchContext,
    },
}

impl std::error::Error for AssetIndexFetchError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteUrl {
    scheme: String,
    authority: String,
    host: String,
    path: String,
}

fn parse_supported_remote_index_url(
    url: &str,
    release_tag: &str,
    context: &AssetIndexFetchContext,
) -> Result<RemoteUrl, AssetIndexFetchError> {
    parse_supported_remote_url(url, release_tag, false, context)
}

fn parse_supported_download_index_url(
    url: &str,
    release_tag: &str,
    context: &AssetIndexFetchContext,
) -> Result<RemoteUrl, AssetIndexFetchError> {
    parse_supported_remote_url(url, release_tag, true, context)
}

fn parse_supported_remote_url(
    url: &str,
    release_tag: &str,
    allow_github_asset_redirect_host: bool,
    context: &AssetIndexFetchContext,
) -> Result<RemoteUrl, AssetIndexFetchError> {
    let parsed = parse_url(url, context)?;
    match parsed.scheme.as_str() {
        "https" if is_github_release_asset_index_url(&parsed, release_tag) => Ok(parsed),
        "http" if is_local_fixture_host(&parsed.host) => Ok(parsed),
        "https" if allow_github_asset_redirect_host && is_github_asset_redirect_host(&parsed.host) => {
            Ok(parsed)
        }
        "https" | "http" => Err(AssetIndexFetchError::UnsupportedUrl {
            url: url.to_owned(),
            reason: format!(
                "release asset index URL must be the pinned {} GitHub release index or a local test fixture",
                crate::release_urls::release_repository()
            ),
            context: context.clone(),
        }),
        _ => Err(AssetIndexFetchError::UnsupportedUrl {
            url: url.to_owned(),
            reason: "release asset index URL must use file://, https:// release assets, or local http:// test fixtures".to_owned(),
            context: context.clone(),
        }),
    }
}

fn parse_url(
    url: &str,
    context: &AssetIndexFetchContext,
) -> Result<RemoteUrl, AssetIndexFetchError> {
    let (scheme, rest) =
        url.split_once("://")
            .ok_or_else(|| AssetIndexFetchError::UnsupportedUrl {
                url: url.to_owned(),
                reason: "release asset index URL must include a URL scheme".to_owned(),
                context: context.clone(),
            })?;
    let scheme = scheme.to_ascii_lowercase();
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, format!("/{path}")),
        None => (rest, "/".to_owned()),
    };
    if authority.is_empty() || authority.contains('@') {
        return Err(AssetIndexFetchError::UnsupportedUrl {
            url: url.to_owned(),
            reason: "release asset index URL has unsupported authority".to_owned(),
            context: context.clone(),
        });
    }
    let host = parse_authority_host(authority, url, context)?;
    if host.is_empty() {
        return Err(AssetIndexFetchError::UnsupportedUrl {
            url: url.to_owned(),
            reason: "release asset index URL has empty host".to_owned(),
            context: context.clone(),
        });
    }
    Ok(RemoteUrl {
        scheme,
        authority: authority.to_ascii_lowercase(),
        host,
        path,
    })
}

fn parse_authority_host(
    authority: &str,
    url: &str,
    context: &AssetIndexFetchContext,
) -> Result<String, AssetIndexFetchError> {
    if let Some(rest) = authority.strip_prefix('[') {
        let Some((host, suffix)) = rest.split_once(']') else {
            return Err(AssetIndexFetchError::UnsupportedUrl {
                url: url.to_owned(),
                reason: "release asset index URL has malformed bracketed host".to_owned(),
                context: context.clone(),
            });
        };
        if !suffix.is_empty() && !suffix.starts_with(':') {
            return Err(AssetIndexFetchError::UnsupportedUrl {
                url: url.to_owned(),
                reason: "release asset index URL has unsupported authority".to_owned(),
                context: context.clone(),
            });
        }
        return Ok(host.to_ascii_lowercase());
    }
    if authority.contains('[') || authority.contains(']') {
        return Err(AssetIndexFetchError::UnsupportedUrl {
            url: url.to_owned(),
            reason: "release asset index URL has malformed bracketed host".to_owned(),
            context: context.clone(),
        });
    }
    Ok(authority
        .rsplit_once(':')
        .map(|(host, _)| host)
        .unwrap_or(authority)
        .to_ascii_lowercase())
}

fn validate_final_index_url(
    initial: &RemoteUrl,
    final_url: &RemoteUrl,
    context: &AssetIndexFetchContext,
) -> Result<(), AssetIndexFetchError> {
    if is_local_fixture_host(&initial.host) {
        if initial.authority == final_url.authority {
            return Ok(());
        }
    } else if is_github_release_asset_index_url(initial, &context.release_tag)
        && (is_github_release_asset_index_url(final_url, &context.release_tag)
            || is_github_asset_redirect_host(&final_url.host))
    {
        return Ok(());
    }
    Err(AssetIndexFetchError::RedirectUnsupported {
        initial: initial.authority.clone(),
        final_url: final_url.authority.clone(),
        context: context.clone(),
    })
}

fn is_github_release_asset_index_url(url: &RemoteUrl, release_tag: &str) -> bool {
    let release_path = crate::release_urls::release_asset_path(release_tag, ASSET_INDEX_NAME);
    url.scheme == "https"
        && url.host == "github.com"
        && (url.path == release_path || url.path == format!("{release_path}.sha256"))
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

fn command_stderr_text(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_owned()
}
