//! Validated template path types.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::TemplateStoreError;

/// Validated in-guest mount destination for a pmem layer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GuestMountPath(PathBuf);

impl GuestMountPath {
    /// Parse a guest mount path under `/opt/m80-layers/<name>`.
    pub fn parse(value: &str) -> Result<Self, TemplateStoreError> {
        if value.is_empty() {
            return invalid_path("template.mount_at", "path must not be empty");
        }

        let path = Path::new(value);
        if !path.is_absolute() {
            return invalid_path("template.mount_at", "path must be absolute");
        }
        if shadows_reserved_mount(path) {
            return invalid_path("template.mount_at", "path shadows a reserved mount");
        }

        let raw_components = value.split('/').collect::<Vec<_>>();
        if raw_components.len() != 4
            || raw_components[0] != ""
            || raw_components[1] != "opt"
            || raw_components[2] != "m80-layers"
        {
            return invalid_path(
                "template.mount_at",
                "path must match /opt/m80-layers/<name>",
            );
        }

        let name = raw_components[3];
        if name == "." || name == ".." {
            return invalid_path(
                "template.mount_at",
                "path must not contain . or .. components",
            );
        }
        if name.is_empty() || name.len() > 64 {
            return invalid_path("template.mount_at", "layer name must be 1..=64 characters");
        }
        if !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        {
            return invalid_path(
                "template.mount_at",
                "layer name must contain only [A-Za-z0-9._-]",
            );
        }
        Ok(Self(path.to_path_buf()))
    }

    /// Return the validated guest path.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl Serialize for GuestMountPath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string_lossy())
    }
}

impl<'de> Deserialize<'de> for GuestMountPath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

/// Validated jail-visible path used by a template backing.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JailBackingPath(PathBuf);

impl JailBackingPath {
    /// Parse an absolute jail-visible path with no escaping components.
    pub fn parse(value: impl Into<PathBuf>) -> Result<Self, TemplateStoreError> {
        let value = value.into();
        if !value.is_absolute() {
            return invalid_path("template.jail_backing_path", "path must be absolute");
        }
        if value.components().any(|component| {
            matches!(
                component,
                Component::CurDir | Component::ParentDir | Component::Prefix(_)
            )
        }) {
            return invalid_path(
                "template.jail_backing_path",
                "path must not contain . or .. components",
            );
        }
        Ok(Self(value))
    }

    /// Return the validated path.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl Serialize for JailBackingPath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string_lossy())
    }
}

impl<'de> Deserialize<'de> for JailBackingPath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = PathBuf::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

fn invalid_path<T>(field: &'static str, reason: &'static str) -> Result<T, TemplateStoreError> {
    Err(TemplateStoreError::InvalidValue { field, reason })
}

fn shadows_reserved_mount(path: &Path) -> bool {
    if path == Path::new("/") {
        return true;
    }

    [
        "/proc",
        "/sys",
        "/dev",
        "/etc",
        "/lower",
        "/upper",
        "/merged",
        "/workspace",
        "/snapshot",
    ]
    .iter()
    .map(Path::new)
    .any(|reserved| path == reserved || path.starts_with(reserved))
}
