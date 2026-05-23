use m80_firecracker::FcError;

use crate::release::{VersionIdentity, VersionStatus};

pub(super) fn validate_artifact_url_matches_binary(
    artifact_url: &str,
    identity: &VersionIdentity,
) -> Result<(), FcError> {
    let Some(bundle_tag) = public_release_tag_from_artifact_url(artifact_url) else {
        return Ok(());
    };
    if bundle_tag == "latest" {
        return Err(FcError::Config(
            m80_firecracker::ConfigError::InvalidValue {
                field: "artifact_url",
                reason: latest_artifact_url_reason(),
            },
        ));
    }
    if identity.version_status == VersionStatus::Release
        && identity.release_tag.as_deref() == Some(bundle_tag.as_str())
    {
        return Ok(());
    }
    Err(FcError::Config(
        m80_firecracker::ConfigError::InvalidValue {
            field: "artifact_url",
            reason: quickstart_bundle_mismatch_reason(&bundle_tag, identity),
        },
    ))
}

fn public_release_tag_from_artifact_url(artifact_url: &str) -> Option<String> {
    let latest_prefix = format!(
        "https://github.com{}",
        crate::release_urls::latest_download_path_prefix()
    );
    if artifact_url.strip_prefix(&latest_prefix).is_some() {
        return Some("latest".to_owned());
    }

    let prefix = format!(
        "https://github.com{}",
        crate::release_urls::release_download_path_prefix()
    );
    let tail = artifact_url.strip_prefix(&prefix)?;
    let tag = tail.split('/').next()?;
    if tag.is_empty() {
        None
    } else {
        Some(tag.to_owned())
    }
}

fn latest_artifact_url_reason() -> String {
    format!(
        "GitHub latest artifact URLs are mutable and are not valid m80 quickstart inputs; install the latest release with: curl -fsSL {} | sudo sh",
        crate::release_urls::latest_install_url()
    )
}

fn quickstart_bundle_mismatch_reason(bundle_tag: &str, identity: &VersionIdentity) -> String {
    let install_command = format!(
        "curl -fsSL {} | sudo sh",
        crate::release_urls::release_install_url(bundle_tag)
    );
    match identity.version_status {
        VersionStatus::Dev => format!(
            "GitHub release artifact URL selects {bundle_tag}, but this m80 binary is dev build {}; run this exact pinned release command next: {install_command}",
            identity.binary_version
        ),
        VersionStatus::Mismatch => format!(
            "GitHub release artifact URL selects {bundle_tag}, but this m80 binary was built with mismatched release tag {}; rebuild with matching M80_RELEASE_TAG or install the selected release with: {install_command}",
            identity.binary_version
        ),
        VersionStatus::Release => format!(
            "GitHub release artifact URL selects {bundle_tag}, but this m80 binary is {}; use the matching versioned install.sh with: {install_command}",
            identity.binary_version
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        latest_artifact_url_reason, public_release_tag_from_artifact_url,
        quickstart_bundle_mismatch_reason, validate_artifact_url_matches_binary,
    };
    use crate::release::VersionIdentity;

    #[test]
    fn public_release_artifact_url_must_match_release_binary() {
        let identity = VersionIdentity::from_parts(
            "1.2.3",
            Some("v1.2.3"),
            Some("0123456789abcdef0123456789abcdef01234567"),
        );

        validate_artifact_url_matches_binary(
            "https://github.com/moradology/m80/releases/download/v1.2.3/m80-linux-x86_64.tar.gz",
            &identity,
        )
        .unwrap();
    }

    #[test]
    fn dev_binary_rejects_public_release_artifact_with_repair_command() {
        let identity = VersionIdentity::from_parts("1.2.3", None, None);
        let err = validate_artifact_url_matches_binary(
            "https://github.com/moradology/m80/releases/download/v1.2.3/m80-linux-x86_64.tar.gz",
            &identity,
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("dev build 1.2.3-dev"), "{err}");
        assert!(
            err.contains(
                "run this exact pinned release command next: curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.3/install.sh | sudo sh"
            ),
            "{err}"
        );
    }

    #[test]
    fn public_latest_artifact_url_is_rejected_as_mutable_legacy_quickstart() {
        let identity = VersionIdentity::from_parts("1.2.3", None, None);
        let err = validate_artifact_url_matches_binary(
            "https://github.com/moradology/m80/releases/latest/download/m80-linux-x86_64.tar.gz",
            &identity,
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("latest artifact URLs are mutable"), "{err}");
        assert!(
            err.contains(
                "curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh"
            ),
            "{err}"
        );
    }

    #[test]
    fn release_binary_rejects_different_public_release_artifact() {
        let identity = VersionIdentity::from_parts(
            "1.2.3",
            Some("v1.2.3"),
            Some("0123456789abcdef0123456789abcdef01234567"),
        );
        let err = validate_artifact_url_matches_binary(
            "https://github.com/moradology/m80/releases/download/v9.9.9/m80-linux-x86_64.tar.gz",
            &identity,
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("selects v9.9.9"), "{err}");
        assert!(err.contains("this m80 binary is v1.2.3"), "{err}");
        assert!(
            err.contains("https://github.com/moradology/m80/releases/download/v9.9.9/install.sh"),
            "{err}"
        );
    }

    #[test]
    fn local_artifact_url_remains_operator_test_override_for_dev_builds() {
        let identity = VersionIdentity::from_parts("1.2.3", None, None);

        validate_artifact_url_matches_binary("file:///tmp/m80-linux-x86_64.tar.gz", &identity)
            .unwrap();
        validate_artifact_url_matches_binary(
            "https://example.invalid/m80-linux-x86_64.tar.gz",
            &identity,
        )
        .unwrap();
    }

    #[test]
    fn public_release_tag_extractor_is_scoped_to_m80_github_release_urls() {
        assert_eq!(
            public_release_tag_from_artifact_url(
                "https://github.com/moradology/m80/releases/download/v1.2.3/m80-linux-x86_64.tar.gz"
            )
            .as_deref(),
            Some("v1.2.3")
        );
        assert_eq!(
            public_release_tag_from_artifact_url(
                "https://example.invalid/releases/download/v1.2.3/m80-linux-x86_64.tar.gz"
            ),
            None
        );
        assert_eq!(
            public_release_tag_from_artifact_url(
                "https://github.com/moradology/m80/releases/latest/download/m80-linux-x86_64.tar.gz"
            )
            .as_deref(),
            Some("latest")
        );
    }

    #[test]
    fn mismatch_reason_points_to_pinned_installer() {
        let identity = VersionIdentity::from_parts(
            "1.2.3",
            Some("v1.2.3"),
            Some("0123456789abcdef0123456789abcdef01234567"),
        );
        let reason = quickstart_bundle_mismatch_reason("v9.9.9", &identity);

        assert!(reason.contains("versioned install.sh"), "{reason}");
        assert!(
            reason.contains("/releases/download/v9.9.9/install.sh"),
            "{reason}"
        );
        assert!(
            latest_artifact_url_reason().contains("/releases/latest/download/install.sh | sudo sh")
        );
    }
}
