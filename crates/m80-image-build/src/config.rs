//! TOML config struct for `m80-image-build run --config <path>`.

use std::path::PathBuf;

use serde::Deserialize;

/// Top-level build configuration loaded from an `m80-image-build.toml`.
#[derive(Debug, Deserialize)]
pub struct BuildConfig {
    /// Kernel image configuration.
    pub kernel: KernelConfig,
    /// Source and output rootfs configuration.
    pub rootfs: RootfsConfig,
    /// Guest daemon binary configuration.
    pub guestd: GuestdConfig,
    /// Output artifact directory configuration.
    pub output: OutputConfig,
}

/// Kernel download parameters.
#[derive(Debug, Deserialize)]
pub struct KernelConfig {
    /// Firecracker CI release tag, e.g. `"v1.15.1"`.
    pub version: String,
    /// CPU architecture string, e.g. `"x86_64"` or `"aarch64"`.
    pub arch: String,
}

/// Source rootfs parameters.
#[derive(Debug, Deserialize)]
pub struct RootfsConfig {
    /// Target ext4 size, e.g. `"1GiB"`, `"512MiB"`, `"100KiB"`.
    pub size: String,
    /// Source type: `"firecracker-ci"` (download) or a local path string.
    ///
    /// v0.1 always downloads from firecracker-ci; local-path override is v0.2.
    #[allow(dead_code)]
    pub source: String,
}

/// Guest daemon binary parameters.
#[derive(Debug, Deserialize)]
pub struct GuestdConfig {
    /// Path to the pre-built `m80-guestd` ELF binary on the host.
    pub binary: PathBuf,
}

/// Build output parameters.
#[derive(Debug, Deserialize)]
pub struct OutputConfig {
    /// Directory where artifacts (kernel, rootfs, manifest) are written.
    pub dir: PathBuf,
}

impl BuildConfig {
    /// Load and parse a TOML config from `path`.
    pub fn from_file(path: &std::path::Path) -> anyhow::Result<BuildConfig> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("reading config {}: {}", path.display(), e))?;
        let cfg: BuildConfig = toml::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("parsing config {}: {}", path.display(), e))?;
        Ok(cfg)
    }
}

/// Parse a human-readable size string into bytes.
///
/// Supported suffixes (case-sensitive): `GiB`, `MiB`, `KiB`.
/// The numeric part must be a non-zero positive integer.
///
/// ```
/// use m80_image_build::config::parse_size;
/// assert_eq!(parse_size("1GiB").unwrap(), 1 << 30);
/// assert_eq!(parse_size("512MiB").unwrap(), 512 << 20);
/// assert_eq!(parse_size("100KiB").unwrap(), 100 << 10);
/// ```
pub fn parse_size(s: &str) -> anyhow::Result<u64> {
    if let Some(n) = s.strip_suffix("GiB") {
        let v: u64 = n
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid size '{}': numeric part not a u64", s))?;
        if v == 0 {
            anyhow::bail!("invalid size '{}': must be > 0", s);
        }
        return Ok(v * (1 << 30));
    }
    if let Some(n) = s.strip_suffix("MiB") {
        let v: u64 = n
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid size '{}': numeric part not a u64", s))?;
        if v == 0 {
            anyhow::bail!("invalid size '{}': must be > 0", s);
        }
        return Ok(v * (1 << 20));
    }
    if let Some(n) = s.strip_suffix("KiB") {
        let v: u64 = n
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid size '{}': numeric part not a u64", s))?;
        if v == 0 {
            anyhow::bail!("invalid size '{}': must be > 0", s);
        }
        return Ok(v * (1 << 10));
    }
    anyhow::bail!(
        "invalid size '{}': expected suffix GiB, MiB, or KiB",
        s
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_size_gib() {
        assert_eq!(parse_size("1GiB").unwrap(), 1 << 30);
        assert_eq!(parse_size("2GiB").unwrap(), 2u64 * (1 << 30));
    }

    #[test]
    fn parse_size_mib() {
        assert_eq!(parse_size("512MiB").unwrap(), 512u64 * (1 << 20));
    }

    #[test]
    fn parse_size_kib() {
        assert_eq!(parse_size("100KiB").unwrap(), 100u64 * (1 << 10));
    }

    #[test]
    fn parse_size_rejects_garbage() {
        assert!(parse_size("garbage").is_err());
        assert!(parse_size("1gb").is_err());
        assert!(parse_size("1TB").is_err());
        assert!(parse_size("").is_err());
        assert!(parse_size("0GiB").is_err());
    }

    #[test]
    fn build_config_round_trips_from_toml() {
        let raw = r#"
[kernel]
version = "v1.15.1"
arch = "x86_64"

[rootfs]
size = "1GiB"
source = "firecracker-ci"

[guestd]
binary = "/tmp/m80-guestd"

[output]
dir = "/opt/m80/artifacts"
"#;
        let cfg: BuildConfig = toml::from_str(raw).expect("TOML parse failed");
        assert_eq!(cfg.kernel.version, "v1.15.1");
        assert_eq!(cfg.kernel.arch, "x86_64");
        assert_eq!(cfg.rootfs.size, "1GiB");
        assert_eq!(cfg.rootfs.source, "firecracker-ci");
        assert_eq!(cfg.guestd.binary, std::path::PathBuf::from("/tmp/m80-guestd"));
        assert_eq!(cfg.output.dir, std::path::PathBuf::from("/opt/m80/artifacts"));
    }
}
