use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::super::super::{InstallPlan, SourceKind, SourcePlan};

pub(super) fn valid_material() -> crate::release_asset_index::DirectBundleIndexMaterial {
    crate::release_asset_index::DirectBundleIndexMaterial {
        release_tag: "v0.0.0".to_owned(),
        bundle_name: "m80-linux-x86_64.tar.gz".to_owned(),
        bundle_url: crate::release_urls::release_asset_url("v0.0.0", "m80-linux-x86_64.tar.gz"),
        bundle_sha256: "a".repeat(64),
        bundle_size_bytes: 42,
        metadata_name: "m80-linux-x86_64.bundle.json".to_owned(),
        metadata_sha256: "b".repeat(64),
        checksum_name: "m80-linux-x86_64.tar.gz.sha256".to_owned(),
        attestation_name: Some("m80-release-integrity.attestation.jsonl".to_owned()),
        target: "linux-x86_64".to_owned(),
        image_kind: "minimal".to_owned(),
        m80_version: "v0.0.0".to_owned(),
        index_url: crate::release_urls::release_asset_url("v0.0.0", "m80-release-assets.json"),
        index_checksum_url: crate::release_urls::release_asset_url(
            "v0.0.0",
            "m80-release-assets.json.sha256",
        ),
        index_expected_sha256: "c".repeat(64),
        index_observed_sha256: "c".repeat(64),
    }
}

pub(super) fn official_release_plan(install_root: &Path) -> InstallPlan {
    let bundle_url = crate::release_urls::release_asset_url("v0.0.0", "m80-linux-x86_64.tar.gz");
    InstallPlan {
        dry_run: false,
        install_root: install_root.display().to_string(),
        bin_dir: install_root.join("bin").display().to_string(),
        active_version_dir: Some(install_root.join("versions/v0.0.0").display().to_string()),
        active_pointer: install_root.join("active").display().to_string(),
        active_pointer_changed: false,
        repair_stale_install_lock: false,
        adopt_existing_config: false,
        source: SourcePlan {
            kind: SourceKind::BundleUrl,
            selector: bundle_url.clone(),
            release_tag: Some("v0.0.0".to_owned()),
            bundle_url: Some(bundle_url.clone()),
        },
        bundle_url: Some(bundle_url),
        default_profile: install_root
            .join("profiles/default.toml")
            .display()
            .to_string(),
        host_binaries_manifest: Some(
            install_root
                .join("versions/v0.0.0/artifacts/host-binaries.manifest.json")
                .display()
                .to_string(),
        ),
        profile_written: false,
        next_command: "m80 run -- echo hello".to_owned(),
        binary_version: "v0.0.0".to_owned(),
        binary_release_tag: Some("v0.0.0".to_owned()),
        version_status: "release".to_owned(),
    }
}

pub(super) fn write_fake_curl(bin_dir: &Path) -> PathBuf {
    let path = bin_dir.join("curl");
    fs::write(
        &path,
        r#"#!/bin/sh
set -eu
out=
url=
while [ "$#" -gt 0 ]; do
    case "$1" in
        -o)
            out=$2
            shift 2
            ;;
        -w|--connect-timeout|--max-time|--proto|--proto-redir)
            shift 2
            ;;
        -fsSL)
            shift
            ;;
        *)
            url=$1
            shift
            ;;
    esac
done
if [ -z "$out" ] || [ -z "$url" ]; then
    echo "fake curl missing -o or url" >&2
    exit 2
fi
printf '%s\n' "$url" >> "$M80_FAKE_CURL_LOG"
name=${url##*/}
src="$M80_FAKE_CURL_MATERIAL_DIR/$name"
final_url=$url
if [ -n "${M80_FAKE_CURL_REDIRECT_MAP:-}" ]; then
    row=$(awk -F '	' -v name="$name" '$1 == name { print; exit }' "$M80_FAKE_CURL_REDIRECT_MAP")
    if [ -n "$row" ]; then
        final_url=$(printf '%s\n' "$row" | awk -F '	' '{ print $2 }')
        source_name=$(printf '%s\n' "$row" | awk -F '	' '{ print $3 }')
        if [ -n "$source_name" ]; then
            src="$M80_FAKE_CURL_MATERIAL_DIR/$source_name"
        fi
    fi
fi
if [ ! -f "$src" ]; then
    echo "fake curl missing $name" >&2
    exit 22
fi
if [ -n "${M80_FAKE_CURL_REQUIRE_GH_MARKER_BEFORE_BUNDLE:-}" ] && [ "$name" = "m80-linux-x86_64.tar.gz" ] && [ ! -f "$M80_FAKE_CURL_REQUIRE_GH_MARKER_BEFORE_BUNDLE" ]; then
    echo "fake curl bundle fetch happened before gh attestation marker" >&2
    exit 23
fi
cp "$src" "$out"
printf '%s' "$final_url"
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
    path
}

pub(super) fn fake_gh_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

pub(super) struct EnvVarGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    pub(super) fn set(key: &'static str, value: &Path) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }

    pub(super) fn set_value(key: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }

    pub(super) fn prepend_path(path: &Path) -> Self {
        let previous = std::env::var_os("PATH");
        let mut value = OsString::from(path.as_os_str());
        if let Some(existing) = &previous {
            value.push(":");
            value.push(existing);
        }
        std::env::set_var("PATH", &value);
        Self {
            key: "PATH",
            previous,
        }
    }

    pub(super) fn prepend_paths(paths: &[PathBuf]) -> Self {
        let previous = std::env::var_os("PATH");
        let mut value = OsString::new();
        for (index, path) in paths.iter().enumerate() {
            if index > 0 {
                value.push(":");
            }
            value.push(path.as_os_str());
        }
        if let Some(existing) = &previous {
            if !paths.is_empty() {
                value.push(":");
            }
            value.push(existing);
        }
        std::env::set_var("PATH", &value);
        Self {
            key: "PATH",
            previous,
        }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}
