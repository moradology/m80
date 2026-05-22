use serde::{de::DeserializeOwned, Deserialize};
use serde_json::Value;

use super::{parse_rfc3339_utc, FreshnessMetadataError};

const SAFETY_FLOOR_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SafetyFloor {
    minimum_safe_tag: Option<String>,
    yanked_tags: Vec<String>,
}

impl SafetyFloor {
    pub(crate) fn minimum_safe_tag(&self) -> Option<&str> {
        self.minimum_safe_tag.as_deref()
    }

    pub(crate) fn yanked_tags(&self) -> &[String] {
        &self.yanked_tags
    }

    pub(crate) fn is_yanked(&self, tag: &str) -> bool {
        self.yanked_tags.iter().any(|candidate| candidate == tag)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SafetyFloorArtifact {
    schema_version: u32,
    published_at: String,
    minimum_safe_tag: Value,
    yanked_releases: Vec<YankedReleaseArtifact>,
}

impl SafetyFloorArtifact {
    pub(super) fn into_floor(self) -> Result<SafetyFloor, FreshnessMetadataError> {
        if self.schema_version != SAFETY_FLOOR_SCHEMA_VERSION {
            return Err(FreshnessMetadataError::UnsupportedSafetyFloorSchema {
                expected: SAFETY_FLOOR_SCHEMA_VERSION,
                actual: self.schema_version,
            });
        }
        require_published_at("safety_floor.published_at", &self.published_at)?;
        let minimum_safe_tag = if self.minimum_safe_tag.is_null() {
            None
        } else {
            Some(
                parse_value::<MinimumSafeTagArtifact>(
                    "safety_floor.minimum_safe_tag",
                    self.minimum_safe_tag,
                )?
                .into_tag()?,
            )
        };
        let mut yanked_tags = Vec::with_capacity(self.yanked_releases.len());
        for release in self.yanked_releases {
            yanked_tags.push(release.into_tag()?);
        }
        Ok(SafetyFloor {
            minimum_safe_tag,
            yanked_tags,
        })
    }
}

fn parse_value<T>(field: &'static str, value: Value) -> Result<T, FreshnessMetadataError>
where
    T: DeserializeOwned,
{
    serde_json::from_value(value).map_err(|source| FreshnessMetadataError::Json {
        detail: format!("{field}: {source}"),
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MinimumSafeTagArtifact {
    tag: String,
    reason: String,
    advisory_url: Option<String>,
    issue_id: Option<String>,
    replacement_command: String,
}

impl MinimumSafeTagArtifact {
    fn into_tag(self) -> Result<String, FreshnessMetadataError> {
        require_stable_tag("safety_floor.minimum_safe_tag.tag", &self.tag)?;
        require_nonempty("safety_floor.minimum_safe_tag.reason", &self.reason)?;
        require_evidence_ref(
            "safety_floor.minimum_safe_tag",
            self.advisory_url.as_deref(),
            self.issue_id.as_deref(),
        )?;
        require_pinned_install_command(
            "safety_floor.minimum_safe_tag.replacement_command",
            &self.replacement_command,
        )?;
        Ok(self.tag)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct YankedReleaseArtifact {
    tag: String,
    reason: String,
    advisory_url: Option<String>,
    issue_id: Option<String>,
    published_at: String,
    replacement_command: Option<String>,
    no_replacement_reason: Option<String>,
}

impl YankedReleaseArtifact {
    fn into_tag(self) -> Result<String, FreshnessMetadataError> {
        require_stable_tag("safety_floor.yanked_releases.tag", &self.tag)?;
        require_nonempty("safety_floor.yanked_releases.reason", &self.reason)?;
        require_published_at(
            "safety_floor.yanked_releases.published_at",
            &self.published_at,
        )?;
        require_evidence_ref(
            "safety_floor.yanked_releases",
            self.advisory_url.as_deref(),
            self.issue_id.as_deref(),
        )?;
        let replacement_command = nonempty_option(
            "safety_floor.yanked_releases.replacement_command",
            self.replacement_command.as_deref(),
        )?;
        let no_replacement_reason = nonempty_option(
            "safety_floor.yanked_releases.no_replacement_reason",
            self.no_replacement_reason.as_deref(),
        )?;
        match (replacement_command, no_replacement_reason) {
            (Some(command), None) => {
                require_pinned_install_command(
                    "safety_floor.yanked_releases.replacement_command",
                    command,
                )?;
            }
            (None, Some(_reason)) => {}
            (Some(_), Some(_)) => {
                return Err(FreshnessMetadataError::ConflictingSafetyReplacement {
                    field: "safety_floor.yanked_releases",
                    tag: self.tag,
                });
            }
            (None, None) => {
                return Err(FreshnessMetadataError::MissingSafetyReplacement {
                    field: "safety_floor.yanked_releases",
                    tag: self.tag,
                });
            }
        }
        Ok(self.tag)
    }
}

fn require_stable_tag(field: &'static str, tag: &str) -> Result<(), FreshnessMetadataError> {
    if crate::release_policy::is_stable_release_tag(tag) {
        Ok(())
    } else {
        Err(FreshnessMetadataError::MalformedSafetyTag {
            field,
            tag: tag.to_owned(),
        })
    }
}

fn require_published_at(field: &'static str, value: &str) -> Result<(), FreshnessMetadataError> {
    parse_rfc3339_utc(value).map(|_| ()).map_err(|_| {
        FreshnessMetadataError::MalformedSafetyPublishedAt {
            field,
            value: value.to_owned(),
        }
    })
}

fn require_nonempty(field: &'static str, value: &str) -> Result<(), FreshnessMetadataError> {
    if value.is_empty() {
        Err(FreshnessMetadataError::EmptyField { field })
    } else {
        Ok(())
    }
}

fn nonempty_option<'a>(
    field: &'static str,
    value: Option<&'a str>,
) -> Result<Option<&'a str>, FreshnessMetadataError> {
    match value {
        Some("") => Err(FreshnessMetadataError::EmptyField { field }),
        Some(value) => Ok(Some(value)),
        None => Ok(None),
    }
}

fn require_evidence_ref(
    field: &'static str,
    advisory_url: Option<&str>,
    issue_id: Option<&str>,
) -> Result<(), FreshnessMetadataError> {
    let advisory_url = nonempty_option("safety_floor.advisory_url", advisory_url)?;
    let issue_id = nonempty_option("safety_floor.issue_id", issue_id)?;
    if advisory_url.is_some() || issue_id.is_some() {
        Ok(())
    } else {
        Err(FreshnessMetadataError::MissingSafetyEvidence { field })
    }
}

fn require_pinned_install_command(
    field: &'static str,
    command: &str,
) -> Result<(), FreshnessMetadataError> {
    let Some(url) = command
        .strip_prefix("curl -fsSL ")
        .and_then(|suffix| suffix.strip_suffix(" | sudo sh"))
    else {
        return Err(FreshnessMetadataError::MalformedSafetyReplacementCommand {
            field,
            command: command.to_owned(),
        });
    };
    let release_prefix = format!(
        "https://github.com/{}/releases/download/",
        crate::release_urls::release_repository()
    );
    let Some(tag) = url
        .strip_prefix(&release_prefix)
        .and_then(|suffix| suffix.strip_suffix("/install.sh"))
    else {
        return Err(FreshnessMetadataError::MalformedSafetyReplacementCommand {
            field,
            command: command.to_owned(),
        });
    };
    if crate::release_policy::is_stable_release_tag(tag)
        && url == crate::release_urls::release_install_url(tag)
    {
        Ok(())
    } else {
        Err(FreshnessMetadataError::MalformedSafetyReplacementCommand {
            field,
            command: command.to_owned(),
        })
    }
}
