//! Installed-version freshness metadata parsing and comparison.

#![cfg_attr(not(test), allow(dead_code))]

use std::fmt;

use serde::Deserialize;

mod safety;

pub(crate) use safety::SafetyFloor;
use safety::SafetyFloorArtifact;

const FRESHNESS_STATUS_SCHEMA_VERSION: u32 = 1;
const LATEST_STATUS_STALE_AFTER_SECONDS: i64 = 48 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LatestFreshnessMetadata {
    repository: String,
    latest_tag: String,
    published_at: UnixSeconds,
    safety_floor: SafetyFloor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct UnixSeconds(i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LatestMetadata<'a> {
    Available(&'a LatestFreshnessMetadata),
    UnknownOffline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveInstallVersion<'a> {
    Release { tag: &'a str },
    Prerelease { tag: &'a str },
    Ineligible { tag: &'a str },
    LocalDev,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FreshnessState {
    Current,
    Outdated,
    UnknownOffline,
    StaleLatestMetadata,
    PrereleaseActive,
    IneligibleActive,
    LocalDevActive,
}

impl FreshnessState {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Outdated => "outdated",
            Self::UnknownOffline => "unknown_offline",
            Self::StaleLatestMetadata => "stale_latest_metadata",
            Self::PrereleaseActive => "prerelease_active",
            Self::IneligibleActive => "ineligible_active",
            Self::LocalDevActive => "local_dev_active",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FreshnessMetadataError {
    Json { detail: String },
    UnsupportedSchema { expected: u32, actual: u32 },
    NotBounded,
    WrongRepository { expected: String, actual: String },
    MissingLatestTag,
    MalformedLatestTag { tag: String },
    MissingPublishedAt,
    MalformedPublishedAt { value: String },
    EmptyField { field: &'static str },
    EmptyCollection { field: &'static str },
    MalformedSha256 { field: &'static str, value: String },
    MalformedSafetyTag { field: &'static str, tag: String },
}

impl fmt::Display for FreshnessMetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json { detail } => write!(f, "freshness status JSON malformed: {detail}"),
            Self::UnsupportedSchema { expected, actual } => write!(
                f,
                "unsupported freshness status schema_version: expected {expected}, got {actual}"
            ),
            Self::NotBounded => write!(
                f,
                "freshness status was not produced by the bounded verifier"
            ),
            Self::WrongRepository { expected, actual } => write!(
                f,
                "freshness status repository mismatch: expected {expected}, got {actual}"
            ),
            Self::MissingLatestTag => write!(f, "freshness status missing resolved_tag"),
            Self::MalformedLatestTag { tag } => {
                write!(f, "freshness status resolved_tag is not stable: {tag}")
            }
            Self::MissingPublishedAt => write!(f, "freshness status missing published_at"),
            Self::MalformedPublishedAt { value } => {
                write!(
                    f,
                    "freshness status published_at is not RFC3339 UTC: {value}"
                )
            }
            Self::EmptyField { field } => write!(f, "freshness status field is empty: {field}"),
            Self::EmptyCollection { field } => {
                write!(f, "freshness status collection is empty: {field}")
            }
            Self::MalformedSha256 { field, value } => {
                write!(f, "freshness status {field} is not sha256: {value}")
            }
            Self::MalformedSafetyTag { field, tag } => {
                write!(
                    f,
                    "freshness status {field} is not a stable release tag: {tag}"
                )
            }
        }
    }
}

impl std::error::Error for FreshnessMetadataError {}

pub(crate) fn read_freshness_status_artifact_json(
    input: &str,
) -> Result<LatestFreshnessMetadata, FreshnessMetadataError> {
    let artifact: FreshnessStatusArtifact =
        serde_json::from_str(input).map_err(|source| FreshnessMetadataError::Json {
            detail: source.to_string(),
        })?;
    artifact.into_metadata()
}

pub(crate) fn compare_freshness(
    active: ActiveInstallVersion<'_>,
    latest: LatestMetadata<'_>,
    now: UnixSeconds,
) -> FreshnessState {
    match active {
        ActiveInstallVersion::LocalDev => FreshnessState::LocalDevActive,
        ActiveInstallVersion::Prerelease { tag } => {
            let _ = tag;
            FreshnessState::PrereleaseActive
        }
        ActiveInstallVersion::Ineligible { tag } => {
            let _ = tag;
            FreshnessState::IneligibleActive
        }
        ActiveInstallVersion::Release { tag } => match latest {
            LatestMetadata::UnknownOffline => FreshnessState::UnknownOffline,
            LatestMetadata::Available(metadata) => {
                if metadata.is_stale_at(now) {
                    FreshnessState::StaleLatestMetadata
                } else if tag == metadata.latest_tag {
                    FreshnessState::Current
                } else {
                    FreshnessState::Outdated
                }
            }
        },
    }
}

pub(crate) fn latest_status_max_age_seconds() -> u64 {
    LATEST_STATUS_STALE_AFTER_SECONDS as u64
}

impl LatestFreshnessMetadata {
    pub(crate) fn repository(&self) -> &str {
        &self.repository
    }

    pub(crate) fn latest_tag(&self) -> &str {
        &self.latest_tag
    }

    pub(crate) fn published_at(&self) -> UnixSeconds {
        self.published_at
    }

    pub(crate) fn safety_floor(&self) -> &SafetyFloor {
        &self.safety_floor
    }

    pub(crate) fn is_stale_at(&self, now: UnixSeconds) -> bool {
        now.0.saturating_sub(self.published_at.0) > LATEST_STATUS_STALE_AFTER_SECONDS
    }
}

impl UnixSeconds {
    pub(crate) const fn new(value: i64) -> Self {
        Self(value)
    }

    pub(crate) const fn as_i64(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FreshnessStatusArtifact {
    schema_version: u32,
    freshness_network_bounded: bool,
    repository: String,
    resolved_tag: Option<String>,
    published_at: Option<String>,
    fetch_policy: FreshnessFetchPolicy,
    checked_urls: Vec<CheckedFreshnessUrl>,
    public_assets: Vec<PublicFreshnessAsset>,
    safety_floor: Option<SafetyFloorArtifact>,
}

impl FreshnessStatusArtifact {
    fn into_metadata(self) -> Result<LatestFreshnessMetadata, FreshnessMetadataError> {
        if self.schema_version != FRESHNESS_STATUS_SCHEMA_VERSION {
            return Err(FreshnessMetadataError::UnsupportedSchema {
                expected: FRESHNESS_STATUS_SCHEMA_VERSION,
                actual: self.schema_version,
            });
        }
        if !self.freshness_network_bounded {
            return Err(FreshnessMetadataError::NotBounded);
        }
        let expected_repository = crate::release_urls::release_repository();
        if self.repository != expected_repository {
            return Err(FreshnessMetadataError::WrongRepository {
                expected: expected_repository,
                actual: self.repository,
            });
        }
        let latest_tag = self
            .resolved_tag
            .ok_or(FreshnessMetadataError::MissingLatestTag)?;
        if !crate::release_policy::is_stable_release_tag(&latest_tag) {
            return Err(FreshnessMetadataError::MalformedLatestTag { tag: latest_tag });
        }
        let published_at = self
            .published_at
            .ok_or(FreshnessMetadataError::MissingPublishedAt)?;
        let published_at = parse_rfc3339_utc(&published_at)?;

        self.fetch_policy.validate()?;
        require_nonempty_collection("checked_urls", &self.checked_urls)?;
        for row in &self.checked_urls {
            row.validate()?;
        }
        require_nonempty_collection("public_assets", &self.public_assets)?;
        for row in &self.public_assets {
            row.validate()?;
        }

        Ok(LatestFreshnessMetadata {
            repository: expected_repository,
            latest_tag,
            published_at,
            safety_floor: self
                .safety_floor
                .map(SafetyFloorArtifact::into_floor)
                .transpose()?
                .unwrap_or_default(),
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FreshnessFetchPolicy {
    connect_timeout_seconds: u64,
    max_time_seconds: u64,
    retry_count: u64,
    retry_delay_seconds: u64,
}

impl FreshnessFetchPolicy {
    fn validate(&self) -> Result<(), FreshnessMetadataError> {
        require_nonzero(
            "fetch_policy.connect_timeout_seconds",
            self.connect_timeout_seconds,
        )?;
        require_nonzero("fetch_policy.max_time_seconds", self.max_time_seconds)?;
        let _ = self.retry_count;
        let _ = self.retry_delay_seconds;
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckedFreshnessUrl {
    role: String,
    url: String,
    release_tag: String,
    asset_name: String,
    sources: Vec<String>,
    size_bytes: Option<u64>,
    sha256: Option<String>,
}

impl CheckedFreshnessUrl {
    fn validate(&self) -> Result<(), FreshnessMetadataError> {
        require_nonempty("checked_urls.role", &self.role)?;
        require_nonempty("checked_urls.url", &self.url)?;
        require_nonempty("checked_urls.release_tag", &self.release_tag)?;
        require_nonempty("checked_urls.asset_name", &self.asset_name)?;
        require_nonempty_collection("checked_urls.sources", &self.sources)?;
        if let Some(size) = self.size_bytes {
            require_nonzero("checked_urls.size_bytes", size)?;
        }
        if let Some(sha256) = &self.sha256 {
            require_sha256("checked_urls.sha256", sha256)?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicFreshnessAsset {
    name: String,
    role: String,
    url: String,
    release_tag: String,
    size_bytes: Option<u64>,
    sha256: String,
}

impl PublicFreshnessAsset {
    fn validate(&self) -> Result<(), FreshnessMetadataError> {
        require_nonempty("public_assets.name", &self.name)?;
        require_nonempty("public_assets.role", &self.role)?;
        require_nonempty("public_assets.url", &self.url)?;
        require_nonempty("public_assets.release_tag", &self.release_tag)?;
        if let Some(size) = self.size_bytes {
            require_nonzero("public_assets.size_bytes", size)?;
        }
        require_sha256("public_assets.sha256", &self.sha256)
    }
}

fn require_nonempty(field: &'static str, value: &str) -> Result<(), FreshnessMetadataError> {
    if value.is_empty() {
        Err(FreshnessMetadataError::EmptyField { field })
    } else {
        Ok(())
    }
}

fn require_nonempty_collection<T>(
    field: &'static str,
    value: &[T],
) -> Result<(), FreshnessMetadataError> {
    if value.is_empty() {
        Err(FreshnessMetadataError::EmptyCollection { field })
    } else {
        Ok(())
    }
}

fn require_nonzero(field: &'static str, value: u64) -> Result<(), FreshnessMetadataError> {
    if value == 0 {
        Err(FreshnessMetadataError::EmptyField { field })
    } else {
        Ok(())
    }
}

fn require_sha256(field: &'static str, value: &str) -> Result<(), FreshnessMetadataError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(FreshnessMetadataError::MalformedSha256 {
            field,
            value: value.to_owned(),
        })
    }
}

fn parse_rfc3339_utc(input: &str) -> Result<UnixSeconds, FreshnessMetadataError> {
    let bytes = input.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        return Err(FreshnessMetadataError::MalformedPublishedAt {
            value: input.to_owned(),
        });
    }

    let year = parse_digits(bytes, 0, 4, input)? as i32;
    let month = parse_digits(bytes, 5, 7, input)?;
    let day = parse_digits(bytes, 8, 10, input)?;
    let hour = parse_digits(bytes, 11, 13, input)?;
    let minute = parse_digits(bytes, 14, 16, input)?;
    let second = parse_digits(bytes, 17, 19, input)?;

    if month == 0
        || month > 12
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return Err(FreshnessMetadataError::MalformedPublishedAt {
            value: input.to_owned(),
        });
    }

    let days = days_from_civil(year, month, day);
    Ok(UnixSeconds(
        days * 86_400 + i64::from(hour * 3_600 + minute * 60 + second),
    ))
}

fn parse_digits(
    bytes: &[u8],
    start: usize,
    end: usize,
    original: &str,
) -> Result<u32, FreshnessMetadataError> {
    let mut value = 0_u32;
    for byte in &bytes[start..end] {
        if !byte.is_ascii_digit() {
            return Err(FreshnessMetadataError::MalformedPublishedAt {
                value: original.to_owned(),
            });
        }
        value = value * 10 + u32::from(byte - b'0');
    }
    Ok(value)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let adjusted_year = year - i32::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let month = month as i32;
    let day = day as i32;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    i64::from(era * 146_097 + day_of_era - 719_468)
}

#[cfg(test)]
mod tests;
