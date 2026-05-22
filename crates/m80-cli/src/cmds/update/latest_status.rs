use std::fs;
use std::path::Path;
use std::process::Command;

use m80_firecracker::{ConfigError, FcError};
use serde::Serialize;

use crate::args::UpdateArgs;
use crate::release_freshness::{read_freshness_status_artifact_json, LatestFreshnessMetadata};

const FRESHNESS_STATUS_ASSET: &str = "m80-latest-freshness-proof.json";

#[derive(Debug, Clone)]
pub(super) enum LatestStatusInput {
    Available {
        source: String,
        metadata: LatestFreshnessMetadata,
        origin: LatestStatusOrigin,
        offline_reason: Option<String>,
    },
    UnknownOffline {
        source: String,
        detail: String,
        origin: LatestStatusOrigin,
        cache_state: LatestStatusCacheState,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum LatestStatusOrigin {
    Remote,
    LocalFile,
    CacheFallback,
    NotRead,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum LatestStatusCacheState {
    NotConfigured,
    NotUsed,
    Fresh,
    Stale,
    Missing,
    Malformed,
}

impl LatestStatusOrigin {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Remote => "remote",
            Self::LocalFile => "local_file",
            Self::CacheFallback => "cache_fallback",
            Self::NotRead => "not_read",
            Self::Unavailable => "unavailable",
        }
    }
}

impl LatestStatusCacheState {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::NotConfigured => "not_configured",
            Self::NotUsed => "not_used",
            Self::Fresh => "fresh",
            Self::Stale => "stale",
            Self::Missing => "missing",
            Self::Malformed => "malformed",
        }
    }
}

pub(super) fn latest_status_input(args: &UpdateArgs) -> Result<LatestStatusInput, FcError> {
    match (&args.latest_status, &args.latest_status_url) {
        (Some(path), Some(url)) => {
            let latest = fetch_latest_status_url(url)?;
            match latest {
                LatestStatusInput::Available { .. } => Ok(latest),
                LatestStatusInput::UnknownOffline { source, detail, .. } => {
                    latest_status_from_cache_after_fetch_failure(path, source, detail)
                }
            }
        }
        (Some(path), None) => read_latest_status_file(path),
        (None, Some(url)) => fetch_latest_status_url(url),
        (None, None) => fetch_latest_status_url(&crate::release_urls::latest_asset_url(
            FRESHNESS_STATUS_ASSET,
        )),
    }
}

fn fetch_latest_status_url(url: &str) -> Result<LatestStatusInput, FcError> {
    let output = match Command::new("curl")
        .arg("-fsSL")
        .arg("--connect-timeout")
        .arg("10")
        .arg("--max-time")
        .arg("120")
        .arg("--retry")
        .arg("2")
        .arg("--retry-delay")
        .arg("1")
        .arg(url)
        .output()
    {
        Ok(output) => output,
        Err(source) => {
            return Ok(LatestStatusInput::UnknownOffline {
                source: url.to_owned(),
                detail: format!("failed to spawn curl for latest status: {source}"),
                origin: LatestStatusOrigin::Unavailable,
                cache_state: LatestStatusCacheState::NotConfigured,
            });
        }
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Ok(LatestStatusInput::UnknownOffline {
            source: url.to_owned(),
            detail: format!(
                "latest status fetch failed: status={} stderr={}",
                output.status,
                stderr.trim()
            ),
            origin: LatestStatusOrigin::Unavailable,
            cache_state: LatestStatusCacheState::NotConfigured,
        });
    }
    let raw = String::from_utf8(output.stdout).map_err(|source| {
        FcError::Config(ConfigError::InvalidValue {
            field: "update.latest_status",
            reason: format!("latest status response was not UTF-8: {source}"),
        })
    })?;
    parse_latest_status(url.to_owned(), raw)
}

fn read_latest_status_file(path: &Path) -> Result<LatestStatusInput, FcError> {
    match fs::read_to_string(path) {
        Ok(raw) => parse_latest_status(format!("file:{}", path.display()), raw)
            .map(|latest| latest.with_origin(LatestStatusOrigin::LocalFile)),
        Err(source) => Ok(LatestStatusInput::UnknownOffline {
            source: format!("file:{}", path.display()),
            detail: format!("latest status artifact unavailable: {source}"),
            origin: LatestStatusOrigin::Unavailable,
            cache_state: LatestStatusCacheState::Missing,
        }),
    }
}

pub(super) fn latest_status_from_cache_after_fetch_failure(
    path: &Path,
    failed_source: String,
    failed_detail: String,
) -> Result<LatestStatusInput, FcError> {
    let cache_source = format!("file:{}", path.display());
    match fs::read_to_string(path) {
        Ok(raw) => match read_freshness_status_artifact_json(&raw) {
            Ok(metadata) => Ok(LatestStatusInput::Available {
                source: cache_source,
                metadata,
                origin: LatestStatusOrigin::CacheFallback,
                offline_reason: Some(format!("{failed_source}: {failed_detail}")),
            }),
            Err(source_err) => Ok(LatestStatusInput::UnknownOffline {
                source: cache_source.clone(),
                detail: format!(
                    "{failed_source}: {failed_detail}; {cache_source}: cached latest status malformed: {source_err}"
                ),
                origin: LatestStatusOrigin::Unavailable,
                cache_state: LatestStatusCacheState::Malformed,
            }),
        },
        Err(source) => Ok(LatestStatusInput::UnknownOffline {
            source: cache_source.clone(),
            detail: format!(
                "{failed_source}: {failed_detail}; {cache_source}: cached latest status unavailable: {source}"
            ),
            origin: LatestStatusOrigin::Unavailable,
            cache_state: LatestStatusCacheState::Missing,
        }),
    }
}

pub(super) fn parse_latest_status(
    source: String,
    raw: String,
) -> Result<LatestStatusInput, FcError> {
    match read_freshness_status_artifact_json(&raw) {
        Ok(metadata) => Ok(LatestStatusInput::Available {
            source,
            metadata,
            origin: LatestStatusOrigin::Remote,
            offline_reason: None,
        }),
        Err(source_err) => Err(FcError::Config(ConfigError::InvalidValue {
            field: "update.latest_status",
            reason: source_err.to_string(),
        })),
    }
}

impl LatestStatusInput {
    fn with_origin(self, origin: LatestStatusOrigin) -> Self {
        match self {
            Self::Available {
                source,
                metadata,
                offline_reason,
                ..
            } => Self::Available {
                source,
                metadata,
                origin,
                offline_reason,
            },
            Self::UnknownOffline {
                source,
                detail,
                cache_state,
                ..
            } => Self::UnknownOffline {
                source,
                detail,
                origin,
                cache_state,
            },
        }
    }
}
