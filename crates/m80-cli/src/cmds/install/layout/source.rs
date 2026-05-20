use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use m80_firecracker::{ConfigError, FcError};

use super::bundle::sha256_file;

const STAGED_BUNDLE_NAME: &str = "bundle.tar.gz";
const STAGED_CHECKSUM_NAME: &str = "bundle.tar.gz.sha256";
const ATTESTATION_GH_ENV: &str = "M80_RELEASE_ATTESTATION_GH";
const DEFAULT_ATTESTATION_GH_BIN: &str = "gh";
const REQUIRED_GH_ATTESTATION_FLAGS: &[&str] = &[
    "--repo",
    "--bundle",
    "--signer-workflow",
    "--cert-oidc-issuer",
    "--source-ref",
    "--source-digest",
    "--deny-self-hosted-runners",
    "--format",
];
const ATTESTATION_VERIFIER_REMEDIATION: &str =
    "Install or upgrade GitHub CLI with attestation support on Linux: https://cli.github.com/packages";

pub(super) fn stage_bundle_source(
    bundle_url: &str,
    staging_dir: &Path,
) -> Result<PathBuf, FcError> {
    if let Some(path) = local_file_url_path(bundle_url)? {
        return stage_local_bundle(&path, staging_dir);
    }
    stage_remote_bundle(bundle_url, staging_dir)
}

pub(super) fn validate_bundle_source_url(bundle_url: &str) -> Result<(), FcError> {
    if local_file_url_path(bundle_url)?.is_some() {
        return Ok(());
    }
    parse_supported_initial_remote_url(bundle_url).map(|_| ())
}

pub(super) fn preflight_attestation_verifier_for_bundle_url(
    bundle_url: &str,
) -> Result<(), FcError> {
    let gh_bin =
        std::env::var(ATTESTATION_GH_ENV).unwrap_or_else(|_| DEFAULT_ATTESTATION_GH_BIN.to_owned());
    preflight_attestation_verifier_for_bundle_url_with_gh(bundle_url, &gh_bin)
}

pub(super) fn is_fixture_bundle_url(bundle_url: &str) -> Result<bool, FcError> {
    if local_file_url_path(bundle_url)?.is_some() {
        return Ok(true);
    }
    let parsed = parse_supported_initial_remote_url(bundle_url)?;
    Ok(parsed.scheme == "http" && is_local_fixture_host(&parsed.host))
}

fn preflight_attestation_verifier_for_bundle_url_with_gh(
    bundle_url: &str,
    gh_bin: &str,
) -> Result<(), FcError> {
    if local_file_url_path(bundle_url)?.is_some() {
        return Ok(());
    }
    let parsed = parse_supported_initial_remote_url(bundle_url)?;
    if !is_github_release_url(&parsed) {
        return Ok(());
    }
    preflight_gh_attestation_verifier(gh_bin)
}

fn preflight_gh_attestation_verifier(gh_bin: &str) -> Result<(), FcError> {
    let version = run_attestation_probe(gh_bin, &["--version"]).map_err(|source| {
        attestation_verifier_error(format!(
            "release attestation verifier missing: {gh_bin}; signed m80 release installs require `gh attestation verify` before downloading release assets; {ATTESTATION_VERIFIER_REMEDIATION}; spawn error: {source}"
        ))
    })?;
    if !version.status.success() {
        return Err(attestation_verifier_error(format!(
            "release attestation verifier unsupported: {gh_bin}; signed m80 release installs require `gh attestation verify` before downloading release assets; observed version output {}; {ATTESTATION_VERIFIER_REMEDIATION}",
            format_probe_output(&version)
        )));
    }

    let help =
        run_attestation_probe(gh_bin, &["attestation", "verify", "--help"]).map_err(|source| {
            attestation_verifier_error(format!(
                "release attestation verifier missing: {gh_bin}; signed m80 release installs require `gh attestation verify --help` before downloading release assets; observed version output {}; {ATTESTATION_VERIFIER_REMEDIATION}; spawn error: {source}",
                format_probe_output(&version)
            ))
        })?;
    let help_text = raw_command_output_text(&help);
    if !help.status.success() {
        return Err(attestation_verifier_error(format!(
            "release attestation verifier unsupported: {gh_bin}; `gh attestation verify --help` failed; observed version output {}; observed help output {}; signed m80 release installs require GitHub Artifact Attestation verification before downloading release assets; {ATTESTATION_VERIFIER_REMEDIATION}",
            format_probe_output(&version),
            format_probe_output(&help)
        )));
    }

    let missing = REQUIRED_GH_ATTESTATION_FLAGS
        .iter()
        .copied()
        .filter(|flag| !help_text.contains(flag))
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(attestation_verifier_error(format!(
            "release attestation verifier unsupported: {gh_bin}; `gh attestation verify --help` is missing required flag(s): {}; observed version output {}; observed help output {}; signed m80 release installs require these flags before downloading release assets; {ATTESTATION_VERIFIER_REMEDIATION}",
            missing.join(", "),
            format_probe_output(&version),
            format_probe_output(&help)
        )));
    }
    Ok(())
}

fn run_attestation_probe(
    gh_bin: &str,
    args: &[&str],
) -> Result<std::process::Output, std::io::Error> {
    Command::new(gh_bin).args(args).output()
}

fn attestation_verifier_error(reason: String) -> FcError {
    FcError::Config(ConfigError::InvalidValue {
        field: "attestation-verifier",
        reason,
    })
}

fn local_file_url_path(url: &str) -> Result<Option<PathBuf>, FcError> {
    let Some(path) = url.strip_prefix("file://") else {
        return Ok(None);
    };
    let path = PathBuf::from(path);
    if path.is_absolute() {
        Ok(Some(path))
    } else {
        Err(FcError::Config(ConfigError::InvalidValue {
            field: "bundle-url",
            reason: "file:// bundle URL must contain an absolute local path".into(),
        }))
    }
}

fn stage_local_bundle(source: &Path, staging_dir: &Path) -> Result<PathBuf, FcError> {
    if !source.is_file() {
        return Err(FcError::ArtifactMissing {
            path: source.to_path_buf(),
        });
    }
    let staged = staging_dir.join(STAGED_BUNDLE_NAME);
    fs::copy(source, &staged).map_err(|source| FcError::PathIo {
        path: staged.clone(),
        source,
    })?;
    Ok(staged)
}

fn stage_remote_bundle(bundle_url: &str, staging_dir: &Path) -> Result<PathBuf, FcError> {
    let initial = parse_supported_initial_remote_url(bundle_url)?;
    let staged = staging_dir.join(STAGED_BUNDLE_NAME);
    let checksum = staging_dir.join(STAGED_CHECKSUM_NAME);
    let checksum_url = format!("{bundle_url}.sha256");

    let result = (|| {
        let final_url = download_url(bundle_url, &staged, "curl release bundle tarball")?;
        validate_final_url(&initial, &parse_supported_download_url(&final_url)?)?;
        let final_checksum_url =
            download_url(&checksum_url, &checksum, "curl release bundle checksum")?;
        validate_final_url(
            &initial,
            &parse_supported_download_url(&final_checksum_url)?,
        )?;
        verify_staged_bundle_checksum(&staged, &checksum)?;
        Ok(staged.clone())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&staged);
        let _ = fs::remove_file(&checksum);
    }
    result
}

fn download_url(url: &str, dest: &Path, command: &'static str) -> Result<String, FcError> {
    let output = Command::new("curl")
        .arg("-fsSL")
        .arg("--proto")
        .arg("=https,http")
        .arg("--proto-redir")
        .arg("=https,http")
        .arg("-o")
        .arg(dest)
        .arg("-w")
        .arg("%{url_effective}")
        .arg(url)
        .output()
        .map_err(|source| FcError::CommandSpawnFailed { command, source })?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(FcError::CommandFailed {
            command,
            status: output.status,
            output: command_output_text(&output),
        })
    }
}

fn verify_staged_bundle_checksum(bundle: &Path, checksum: &Path) -> Result<(), FcError> {
    let expected = read_expected_sha256(checksum)?;
    let actual = sha256_file(bundle)?;
    if actual == expected {
        Ok(())
    } else {
        Err(FcError::Config(ConfigError::InvalidValue {
            field: "bundle-url.sha256",
            reason: format!("bundle sha256 mismatch: expected {expected}, got {actual}"),
        }))
    }
}

fn read_expected_sha256(checksum_file: &Path) -> Result<String, FcError> {
    let text = fs::read_to_string(checksum_file).map_err(|source| FcError::PathIo {
        path: checksum_file.to_path_buf(),
        source,
    })?;
    let expected = text.split_whitespace().next().ok_or_else(|| {
        FcError::Config(ConfigError::InvalidValue {
            field: "bundle-url.sha256",
            reason: format!("checksum file {} is empty", checksum_file.display()),
        })
    })?;
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "bundle-url.sha256",
            reason: format!(
                "checksum file {} must start with a 64-hex sha256 digest",
                checksum_file.display()
            ),
        }));
    }
    Ok(expected.to_ascii_lowercase())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteUrl {
    scheme: String,
    authority: String,
    host: String,
    path: String,
}

fn parse_supported_initial_remote_url(url: &str) -> Result<RemoteUrl, FcError> {
    parse_supported_remote_url(url, false)
}

fn parse_supported_download_url(url: &str) -> Result<RemoteUrl, FcError> {
    parse_supported_remote_url(url, true)
}

fn parse_supported_remote_url(
    url: &str,
    allow_github_asset_redirect_host: bool,
) -> Result<RemoteUrl, FcError> {
    let parsed = parse_url(url)?;
    match parsed.scheme.as_str() {
        "https" if is_github_release_url(&parsed) => Ok(parsed),
        "http" if is_local_fixture_host(&parsed.host) => Ok(parsed),
        "https"
            if allow_github_asset_redirect_host
                && is_github_asset_redirect_host(&parsed.host) =>
        {
            Ok(parsed)
        }
        "https" | "http" => Err(FcError::Config(ConfigError::InvalidValue {
            field: "bundle-url",
            reason: format!(
                "remote bundle URL must be a {} GitHub release asset or local test fixture: {url}",
                crate::release_urls::release_repository()
            ),
        })),
        _ => Err(FcError::UnsupportedOperation {
            operation: "m80 install",
            reason: "bundle URL must use file://, https:// release assets, or local http:// test fixtures".into(),
        }),
    }
}

fn parse_url(url: &str) -> Result<RemoteUrl, FcError> {
    let (scheme, rest) = url.split_once("://").ok_or_else(|| {
        FcError::UnsupportedOperation {
            operation: "m80 install",
            reason: "bundle URL must use file://, https:// release assets, or local http:// test fixtures".into(),
        }
    })?;
    let scheme = scheme.to_ascii_lowercase();
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, format!("/{path}")),
        None => (rest, "/".to_owned()),
    };
    if authority.is_empty() || authority.contains('@') {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "bundle-url",
            reason: format!("remote bundle URL has unsupported authority: {url}"),
        }));
    }
    let host = parse_authority_host(authority, url)?;
    if host.is_empty() {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "bundle-url",
            reason: format!("remote bundle URL has empty host: {url}"),
        }));
    }
    Ok(RemoteUrl {
        scheme,
        authority: authority.to_ascii_lowercase(),
        host,
        path,
    })
}

fn parse_authority_host(authority: &str, url: &str) -> Result<String, FcError> {
    if let Some(rest) = authority.strip_prefix('[') {
        let Some((host, suffix)) = rest.split_once(']') else {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "bundle-url",
                reason: format!("remote bundle URL has malformed bracketed host: {url}"),
            }));
        };
        if !suffix.is_empty() && !suffix.starts_with(':') {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "bundle-url",
                reason: format!("remote bundle URL has unsupported authority: {url}"),
            }));
        }
        return Ok(host.to_ascii_lowercase());
    }
    if authority.contains('[') || authority.contains(']') {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "bundle-url",
            reason: format!("remote bundle URL has malformed bracketed host: {url}"),
        }));
    }
    Ok(authority
        .rsplit_once(':')
        .map(|(host, _)| host)
        .unwrap_or(authority)
        .to_ascii_lowercase())
}

fn validate_final_url(initial: &RemoteUrl, final_url: &RemoteUrl) -> Result<(), FcError> {
    if is_local_fixture_host(&initial.host) {
        if initial.authority == final_url.authority {
            return Ok(());
        }
    } else if is_github_release_url(initial)
        && (is_github_release_url(final_url) || is_github_asset_redirect_host(&final_url.host))
    {
        return Ok(());
    }
    Err(FcError::Config(ConfigError::InvalidValue {
        field: "bundle-url",
        reason: format!(
            "release bundle download redirected to unsupported host {}; expected {}",
            final_url.authority, initial.authority
        ),
    }))
}

fn is_github_release_url(url: &RemoteUrl) -> bool {
    url.scheme == "https"
        && url.host == "github.com"
        && url
            .path
            .starts_with(&crate::release_urls::release_download_path_prefix())
}

fn is_github_asset_redirect_host(host: &str) -> bool {
    matches!(
        host,
        "objects.githubusercontent.com"
            | "github-releases.githubusercontent.com"
            | "release-assets.githubusercontent.com"
    )
}

fn is_local_fixture_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

fn command_output_text(output: &std::process::Output) -> String {
    let combined = raw_command_output_text(output);
    if combined.is_empty() {
        String::new()
    } else {
        format!(": {combined}")
    }
}

fn raw_command_output_text(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn format_probe_output(output: &std::process::Output) -> String {
    let text = raw_command_output_text(output);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        format!("exit={} <no output>", output.status)
    } else {
        format!("exit={} {}", output.status, trimmed.replace('\n', "\\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_url_rejects_non_release_https_host() {
        let err =
            parse_supported_initial_remote_url("https://example.invalid/m80-linux-x86_64.tar.gz")
                .unwrap_err();
        assert!(
            err.to_string()
                .contains("moradology/m80 GitHub release asset"),
            "{err}"
        );
    }

    #[test]
    fn initial_remote_url_rejects_github_cdn_host() {
        let err = parse_supported_initial_remote_url(
            "https://release-assets.githubusercontent.com/github-production-release-asset/file",
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("moradology/m80 GitHub release asset"),
            "{err}"
        );
    }

    #[test]
    fn local_fixture_redirect_must_stay_on_same_authority() {
        let initial =
            parse_supported_initial_remote_url("http://127.0.0.1:1234/bundle.tar.gz").unwrap();
        let final_url =
            parse_supported_download_url("http://localhost:1234/bundle.tar.gz").unwrap();

        let err = validate_final_url(&initial, &final_url).unwrap_err();
        assert!(
            err.to_string().contains("redirected to unsupported host"),
            "{err}"
        );
    }

    #[test]
    fn local_fixture_ipv6_host_without_port_is_supported() {
        let parsed =
            parse_supported_initial_remote_url("http://[::1]/m80-linux-x86_64.tar.gz").unwrap();

        assert_eq!(parsed.host, "::1");
        assert_eq!(parsed.authority, "[::1]");
    }

    #[test]
    fn attestation_verifier_preflight_ignores_local_fixture_url() {
        preflight_attestation_verifier_for_bundle_url_with_gh(
            "http://127.0.0.1:1234/m80-linux-x86_64.tar.gz",
            "/does/not/exist/gh",
        )
        .unwrap();
    }

    #[test]
    fn attestation_verifier_preflight_accepts_supported_gh_help() {
        let gh = fake_gh_fixture("fake-gh-attestation-supported.sh");

        preflight_attestation_verifier_for_bundle_url_with_gh(
            &crate::release_urls::release_asset_url("v0.0.0", "m80-linux-x86_64.tar.gz"),
            gh.to_str().unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn attestation_verifier_preflight_rejects_help_missing_required_flag() {
        let gh = fake_gh_fixture("fake-gh-attestation-missing-source-digest.sh");

        let err = preflight_attestation_verifier_for_bundle_url_with_gh(
            &crate::release_urls::release_asset_url("v0.0.0", "m80-linux-x86_64.tar.gz"),
            gh.to_str().unwrap(),
        )
        .unwrap_err();

        let message = err.to_string();
        assert!(
            message.contains("release attestation verifier unsupported"),
            "{message}"
        );
        assert!(message.contains("--source-digest"), "{message}");
    }

    fn fake_gh_fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }
}
