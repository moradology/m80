use std::collections::BTreeMap;

const PUBLIC_RELEASE_ROOT: &str =
    include_str!("../../../docs/behaviors/release/public-release-root.env");

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublicReleaseRoot {
    owner: String,
    repo: String,
}

impl PublicReleaseRoot {
    fn repository(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }

    fn release_download_path_prefix(&self) -> String {
        format!("/{}/releases/download/", self.repository())
    }

    fn latest_download_path_prefix(&self) -> String {
        format!("/{}/releases/latest/download/", self.repository())
    }

    fn release_download_base_url(&self, release_tag: &str) -> String {
        format!(
            "https://github.com/{}/releases/download/{release_tag}",
            self.repository()
        )
    }
}

pub(crate) fn release_repository() -> String {
    public_release_root().repository()
}

pub(crate) fn release_asset_url(release_tag: &str, asset_name: &str) -> String {
    format!(
        "{}/{}",
        public_release_root().release_download_base_url(release_tag),
        asset_name
    )
}

pub(crate) fn release_install_url(release_tag: &str) -> String {
    release_asset_url(release_tag, "install.sh")
}

pub(crate) fn release_tag_from_download_url(url: &str) -> Option<&str> {
    let prefix = format!("https://github.com{}", release_download_path_prefix());
    let rest = url.strip_prefix(&prefix)?;
    let (tag, _) = rest.split_once('/')?;
    if tag.is_empty() {
        None
    } else {
        Some(tag)
    }
}

pub(crate) fn latest_install_url() -> String {
    latest_asset_url("install.sh")
}

pub(crate) fn latest_asset_url(asset_name: &str) -> String {
    format!(
        "https://github.com{}{}",
        latest_download_path_prefix(),
        asset_name
    )
}

pub(crate) fn release_download_path_prefix() -> String {
    public_release_root().release_download_path_prefix()
}

pub(crate) fn latest_download_path_prefix() -> String {
    public_release_root().latest_download_path_prefix()
}

pub(crate) fn release_asset_path(release_tag: &str, asset_name: &str) -> String {
    format!(
        "{}{release_tag}/{asset_name}",
        public_release_root().release_download_path_prefix()
    )
}

fn public_release_root() -> PublicReleaseRoot {
    parse_public_release_root(PUBLIC_RELEASE_ROOT)
        .expect("docs/behaviors/release/public-release-root.env must be valid")
}

fn parse_public_release_root(input: &str) -> Result<PublicReleaseRoot, String> {
    let mut values = BTreeMap::new();
    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("malformed public release root line: {line:?}"));
        };
        values.insert(key, value);
    }
    let owner = required_token(&values, "M80_PUBLIC_RELEASE_OWNER")?;
    let repo = required_token(&values, "M80_PUBLIC_RELEASE_REPO")?;
    Ok(PublicReleaseRoot { owner, repo })
}

fn required_token(values: &BTreeMap<&str, &str>, key: &'static str) -> Result<String, String> {
    let value = values
        .get(key)
        .ok_or_else(|| format!("missing {key}"))?
        .to_string();
    if value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
    {
        Ok(value)
    } else {
        Err(format!("{key} must be a GitHub owner/repo token"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_release_root_is_moradology_m80() {
        assert_eq!(release_repository(), "moradology/m80");
        assert_eq!(
            release_asset_url("v0.0.0", "install.sh"),
            "https://github.com/moradology/m80/releases/download/v0.0.0/install.sh"
        );
        assert_eq!(
            release_asset_path("v0.0.0", "m80-release-assets.json"),
            "/moradology/m80/releases/download/v0.0.0/m80-release-assets.json"
        );
        assert_eq!(
            latest_download_path_prefix(),
            "/moradology/m80/releases/latest/download/"
        );
        assert_eq!(
            latest_install_url(),
            "https://github.com/moradology/m80/releases/latest/download/install.sh"
        );
    }

    #[test]
    fn release_tag_from_download_url_accepts_public_pinned_assets() {
        assert_eq!(
            release_tag_from_download_url(
                "https://github.com/moradology/m80/releases/download/v0.2.11/install.sh"
            ),
            Some("v0.2.11")
        );
        assert_eq!(
            release_tag_from_download_url(
                "https://github.com/moradology/m80/releases/latest/download/install.sh"
            ),
            None
        );
        assert_eq!(
            release_tag_from_download_url("file:///tmp/m80-linux-x86_64.tar.gz"),
            None
        );
    }

    #[test]
    fn public_release_root_rejects_slashes_inside_tokens() {
        let err = parse_public_release_root(
            "M80_PUBLIC_RELEASE_OWNER=moradology/other\nM80_PUBLIC_RELEASE_REPO=m80\n",
        )
        .unwrap_err();

        assert!(err.contains("M80_PUBLIC_RELEASE_OWNER"), "{err}");
    }
}
