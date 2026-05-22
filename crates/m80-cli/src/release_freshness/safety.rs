use std::collections::HashSet;

use serde::{de::DeserializeOwned, Deserialize};
use serde_json::Value;

use super::{parse_rfc3339_utc, FreshnessMetadataError, UnixSeconds};

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
    pub(super) fn into_floor(
        self,
        latest_tag: &str,
        latest_published_at: UnixSeconds,
    ) -> Result<SafetyFloor, FreshnessMetadataError> {
        if self.schema_version != SAFETY_FLOOR_SCHEMA_VERSION {
            return Err(FreshnessMetadataError::UnsupportedSafetyFloorSchema {
                expected: SAFETY_FLOOR_SCHEMA_VERSION,
                actual: self.schema_version,
            });
        }
        let published_at = require_published_at("safety_floor.published_at", &self.published_at)?;
        require_not_after(
            "safety_floor.published_at",
            None,
            published_at,
            "published_at",
            latest_published_at,
        )?;
        let minimum_safe_tag = if self.minimum_safe_tag.is_null() {
            None
        } else {
            Some(
                parse_value::<MinimumSafeTagArtifact>(
                    "safety_floor.minimum_safe_tag",
                    self.minimum_safe_tag,
                )?
                .into_rule()?,
            )
        };
        let latest_version = stable_version("resolved_tag", latest_tag)?;
        if let Some(rule) = &minimum_safe_tag {
            let minimum_version = stable_version("safety_floor.minimum_safe_tag.tag", &rule.tag)?;
            if minimum_version > latest_version {
                return Err(FreshnessMetadataError::SafetyFloorContradiction {
                    field: "safety_floor.minimum_safe_tag.tag",
                    tag: rule.tag.clone(),
                    reason: format!("minimum_safe_tag is newer than latest stable {latest_tag}"),
                });
            }
        }

        let mut yanked_tags = Vec::with_capacity(self.yanked_releases.len());
        let mut seen_yanked = HashSet::new();
        let mut yanked_releases = Vec::new();
        for release in self.yanked_releases {
            let release = release.into_rule(published_at)?;
            if !seen_yanked.insert(release.tag.clone()) {
                return Err(FreshnessMetadataError::SafetyFloorContradiction {
                    field: "safety_floor.yanked_releases.tag",
                    tag: release.tag,
                    reason: "duplicate yanked tag".to_owned(),
                });
            }
            yanked_tags.push(release.tag.clone());
            yanked_releases.push(release);
        }
        let yanked_set = yanked_tags.iter().cloned().collect::<HashSet<_>>();
        for release in &yanked_releases {
            if release.tag == latest_tag && release.replacement_tag.is_none() {
                return Err(FreshnessMetadataError::SafetyFloorContradiction {
                    field: "safety_floor.yanked_releases.replacement_command",
                    tag: release.tag.clone(),
                    reason: "latest stable is yanked without a replacement command".to_owned(),
                });
            }
            if let Some(replacement_tag) = &release.replacement_tag {
                require_safe_replacement_tag(
                    replacement_tag,
                    minimum_safe_tag.as_ref(),
                    &yanked_set,
                )?;
            }
        }
        if let Some(rule) = &minimum_safe_tag {
            require_safe_replacement_tag(&rule.replacement_tag, Some(rule), &yanked_set)?;
        }
        Ok(SafetyFloor {
            minimum_safe_tag: minimum_safe_tag.map(|rule| rule.tag),
            yanked_tags,
        })
    }
}

#[derive(Debug)]
struct MinimumSafeRule {
    tag: String,
    replacement_tag: String,
}

#[derive(Debug)]
struct YankedReleaseRule {
    tag: String,
    replacement_tag: Option<String>,
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
    fn into_rule(self) -> Result<MinimumSafeRule, FreshnessMetadataError> {
        require_stable_tag("safety_floor.minimum_safe_tag.tag", &self.tag)?;
        require_nonempty("safety_floor.minimum_safe_tag.reason", &self.reason)?;
        require_evidence_ref(
            "safety_floor.minimum_safe_tag",
            self.advisory_url.as_deref(),
            self.issue_id.as_deref(),
        )?;
        let replacement_tag = require_pinned_install_command(
            "safety_floor.minimum_safe_tag.replacement_command",
            &self.replacement_command,
        )?;
        Ok(MinimumSafeRule {
            tag: self.tag,
            replacement_tag,
        })
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
    fn into_rule(
        self,
        safety_floor_published_at: UnixSeconds,
    ) -> Result<YankedReleaseRule, FreshnessMetadataError> {
        require_stable_tag("safety_floor.yanked_releases.tag", &self.tag)?;
        require_nonempty("safety_floor.yanked_releases.reason", &self.reason)?;
        let published_at = require_published_at(
            "safety_floor.yanked_releases.published_at",
            &self.published_at,
        )?;
        require_not_after(
            "safety_floor.yanked_releases.published_at",
            Some(&self.tag),
            published_at,
            "safety_floor.published_at",
            safety_floor_published_at,
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
        let replacement_tag = match (replacement_command, no_replacement_reason) {
            (Some(command), None) => Some(require_pinned_install_command(
                "safety_floor.yanked_releases.replacement_command",
                command,
            )?),
            (None, Some(_reason)) => None,
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
        };
        Ok(YankedReleaseRule {
            tag: self.tag,
            replacement_tag,
        })
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

fn require_published_at(
    field: &'static str,
    value: &str,
) -> Result<UnixSeconds, FreshnessMetadataError> {
    parse_rfc3339_utc(value).map_err(|_| FreshnessMetadataError::MalformedSafetyPublishedAt {
        field,
        value: value.to_owned(),
    })
}

fn require_not_after(
    field: &'static str,
    tag: Option<&str>,
    value: UnixSeconds,
    reference_field: &'static str,
    reference_value: UnixSeconds,
) -> Result<(), FreshnessMetadataError> {
    if value <= reference_value {
        Ok(())
    } else {
        Err(FreshnessMetadataError::StaleSafetyTimestamp {
            field,
            tag: tag.map(str::to_owned),
            value: value.as_i64(),
            reference_field,
            reference_value: reference_value.as_i64(),
        })
    }
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
) -> Result<String, FreshnessMetadataError> {
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
        Ok(tag.to_owned())
    } else {
        Err(FreshnessMetadataError::MalformedSafetyReplacementCommand {
            field,
            command: command.to_owned(),
        })
    }
}

fn require_safe_replacement_tag(
    replacement_tag: &str,
    minimum_safe_tag: Option<&MinimumSafeRule>,
    yanked_tags: &HashSet<String>,
) -> Result<(), FreshnessMetadataError> {
    if yanked_tags.contains(replacement_tag) {
        return Err(FreshnessMetadataError::SafetyFloorContradiction {
            field: "safety_floor.replacement_command",
            tag: replacement_tag.to_owned(),
            reason: "replacement command targets a yanked release".to_owned(),
        });
    }
    if let Some(rule) = minimum_safe_tag {
        if stable_version("safety_floor.replacement_command", replacement_tag)?
            < stable_version("safety_floor.minimum_safe_tag.tag", &rule.tag)?
        {
            return Err(FreshnessMetadataError::SafetyFloorContradiction {
                field: "safety_floor.replacement_command",
                tag: replacement_tag.to_owned(),
                reason: format!("replacement command is below minimum_safe_tag {}", rule.tag),
            });
        }
    }
    Ok(())
}

fn stable_version(
    field: &'static str,
    tag: &str,
) -> Result<crate::release_policy::StableReleaseTag, FreshnessMetadataError> {
    crate::release_policy::parse_stable_release_tag(tag).map_err(|_| {
        FreshnessMetadataError::MalformedSafetyTag {
            field,
            tag: tag.to_owned(),
        }
    })
}
