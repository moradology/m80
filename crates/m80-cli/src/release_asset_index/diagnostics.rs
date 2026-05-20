use std::fmt;

use super::AssetIndexError;

impl fmt::Display for AssetIndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json { detail } => write!(f, "release asset index JSON is invalid: {detail}"),
            Self::UnsupportedSchema { expected, actual } => write!(
                f,
                "release asset index schema mismatch: expected {expected}, got {actual}"
            ),
            Self::MissingField { field } => {
                write!(f, "release asset index missing required field {field}")
            }
            Self::InvalidField { field, detail } => {
                write!(f, "release asset index invalid field {field}: {detail}")
            }
            Self::MissingDefault {
                os,
                arch,
                image_kind,
                available,
            } => write!(
                f,
                "release asset index has no default bundle for {os}/{arch}/{image_kind}; available: {}",
                available.join(", ")
            ),
            Self::DevBuildSelection { m80_version } => write!(
                f,
                "dev build {m80_version} cannot select release assets from the index; use --bundle-url for a local bundle or install a tagged release binary"
            ),
            Self::MismatchedBuildSelection {
                release_tag,
                m80_version,
            } => write!(
                f,
                "mismatched release build tag {release_tag} with binary version {m80_version} cannot select release assets from the index; rebuild with matching release metadata"
            ),
            Self::WrongTag { index, binary } => write!(
                f,
                "release asset index tag {index} does not match binary tag {binary}; retry pinned release URL {}",
                release_install_url(binary)
            ),
            Self::WrongArchitecture {
                os,
                arch,
                available,
            } => write!(
                f,
                "release asset index has no bundle for requested host tuple os={os} arch={arch}; available tuples: {}; use --bundle-url for an explicit compatible bundle",
                available.join(", ")
            ),
            Self::WrongImageKind {
                os,
                arch,
                image_kind,
                available,
            } => write!(
                f,
                "release asset index has no requested image kind {image_kind} for os={os} arch={arch}; available image kinds: {}; use --bundle-url for an explicit compatible bundle",
                available.join(", ")
            ),
            Self::WrongM80Version {
                expected,
                available,
            } => write!(
                f,
                "release asset index has no bundle for requested m80 version {expected}; available versions: {}; retry pinned release URLs: {}",
                available.join(", "),
                available
                    .iter()
                    .map(|version| release_install_url(version))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::DuplicateDefault {
                os,
                arch,
                image_kind,
                release_tag,
            } => write!(
                f,
                "release asset index has duplicate default bundles for requested release_tag={release_tag} os={os} arch={arch} image_kind={image_kind}; publish exactly one matching row"
            ),
            Self::AssetReleaseTagMismatch { index, asset, name } => write!(
                f,
                "release asset {name} tag {asset} does not match index tag {index}"
            ),
            Self::TargetTupleMismatch {
                name,
                target,
                os,
                arch,
            } => write!(
                f,
                "release asset {name} target {target} does not match os/arch {os}/{arch}"
            ),
        }
    }
}

fn release_install_url(release_tag: &str) -> String {
    format!("https://github.com/moradology/m80/releases/download/{release_tag}/install.sh")
}
