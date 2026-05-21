use std::fs;
use std::process::Command;

use m80_firecracker::{ConfigError, FcError};

use crate::args::UpdateArgs;
use crate::release_freshness::{read_freshness_status_artifact_json, LatestFreshnessMetadata};

const FRESHNESS_STATUS_ASSET: &str = "m80-latest-freshness-proof.json";

#[derive(Debug, Clone)]
pub(super) enum LatestStatusInput {
    Available {
        source: String,
        metadata: LatestFreshnessMetadata,
    },
    UnknownOffline {
        source: String,
        detail: String,
    },
}

pub(super) fn latest_status_input(args: &UpdateArgs) -> Result<LatestStatusInput, FcError> {
    match (&args.latest_status, &args.latest_status_url) {
        (Some(_), Some(_)) => Err(FcError::Config(ConfigError::InvalidValue {
            field: "update.latest_status",
            reason: "--latest-status and --latest-status-url are mutually exclusive".to_owned(),
        })),
        (Some(path), None) => match fs::read_to_string(path) {
            Ok(raw) => parse_latest_status(format!("file:{}", path.display()), raw),
            Err(source) => Ok(LatestStatusInput::UnknownOffline {
                source: format!("file:{}", path.display()),
                detail: format!("latest status artifact unavailable: {source}"),
            }),
        },
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

pub(super) fn parse_latest_status(
    source: String,
    raw: String,
) -> Result<LatestStatusInput, FcError> {
    match read_freshness_status_artifact_json(&raw) {
        Ok(metadata) => Ok(LatestStatusInput::Available { source, metadata }),
        Err(source_err) => Err(FcError::Config(ConfigError::InvalidValue {
            field: "update.latest_status",
            reason: source_err.to_string(),
        })),
    }
}
