use serde::Deserialize;

use super::FreshnessMetadataError;

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
    minimum_safe_tag: Option<String>,
    #[serde(default)]
    yanked_tags: Vec<String>,
}

impl SafetyFloorArtifact {
    pub(super) fn into_floor(self) -> Result<SafetyFloor, FreshnessMetadataError> {
        if let Some(tag) = &self.minimum_safe_tag {
            require_stable_tag("safety_floor.minimum_safe_tag", tag)?;
        }
        for tag in &self.yanked_tags {
            require_stable_tag("safety_floor.yanked_tags", tag)?;
        }
        Ok(SafetyFloor {
            minimum_safe_tag: self.minimum_safe_tag,
            yanked_tags: self.yanked_tags,
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
