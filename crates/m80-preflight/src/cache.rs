//! Boot-scoped preflight cache for expensive binary/artifact checks.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use m80_image_manifest::Manifest;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::artifacts::{discover_kernel, manifest_path_for_rootfs, ArtifactPreflightConfig};
use crate::binary::BinaryDiscoveryConfig;

pub(crate) const ENV_FORCE_PREFLIGHT: &str = "M80_FORCE_PREFLIGHT";
const BOOT_ID_PATH: &str = "/proc/sys/kernel/random/boot_id";
const SENTINEL_DIR: &str = "/run";
const SENTINEL_PREFIX: &str = "m80-preflight-ok-";

#[derive(Debug)]
pub(crate) struct PreflightCache {
    key: Option<PreflightCacheKey>,
    path: Option<PathBuf>,
    hit: Option<PreflightCacheHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreflightCacheHit {
    pub(crate) firecracker_version: String,
    pub(crate) manifest: Manifest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PreflightSentinel {
    key: PreflightCacheKey,
    hit: PreflightCacheHit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PreflightCacheKey {
    boot_id: String,
    expected_firecracker_version: Option<String>,
    kernel_kind: Option<String>,
    firecracker: FileIdentity,
    firecracker_seccomp_filter: FileIdentity,
    jailer: FileIdentity,
    jailer_harden: FileIdentity,
    kernel: FileIdentity,
    rootfs: FileIdentity,
    manifest: FileIdentity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct FileIdentity {
    path: PathBuf,
    dev: u64,
    ino: u64,
    mtime_sec: i64,
    mtime_nsec: i64,
    size: u64,
}

impl PreflightCache {
    pub(crate) fn load(
        binary_config: &BinaryDiscoveryConfig,
        artifact_config: &ArtifactPreflightConfig,
    ) -> Self {
        if std::env::var_os(ENV_FORCE_PREFLIGHT).is_some() {
            return Self::disabled();
        }
        Self::load_from_dir(
            Path::new(SENTINEL_DIR),
            Path::new(BOOT_ID_PATH),
            binary_config,
            artifact_config,
        )
    }

    pub(crate) fn hit(&self) -> Option<&PreflightCacheHit> {
        self.hit.as_ref()
    }

    pub(crate) fn store(&self, firecracker_version: &str, manifest: &Manifest) {
        let (Some(key), Some(path)) = (&self.key, &self.path) else {
            return;
        };
        if self.hit.is_some() {
            return;
        }
        let sentinel = PreflightSentinel {
            key: key.clone(),
            hit: PreflightCacheHit {
                firecracker_version: firecracker_version.to_owned(),
                manifest: manifest.clone(),
            },
        };
        let Ok(bytes) = serde_json::to_vec(&sentinel) else {
            return;
        };
        if let Err(err) = fs::write(path, bytes) {
            tracing::debug!(path = %path.display(), error = %err, "failed to write preflight cache sentinel");
        }
    }

    fn load_from_dir(
        sentinel_dir: &Path,
        boot_id_path: &Path,
        binary_config: &BinaryDiscoveryConfig,
        artifact_config: &ArtifactPreflightConfig,
    ) -> Self {
        let Ok(key) = PreflightCacheKey::from_configs(boot_id_path, binary_config, artifact_config)
        else {
            return Self::disabled();
        };
        let path = sentinel_dir.join(format!("{}{}", SENTINEL_PREFIX, key.digest()));
        let hit = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<PreflightSentinel>(&bytes).ok())
            .and_then(|sentinel| (sentinel.key == key).then_some(sentinel.hit));

        Self {
            key: Some(key),
            path: Some(path),
            hit,
        }
    }

    fn disabled() -> Self {
        Self {
            key: None,
            path: None,
            hit: None,
        }
    }
}

impl PreflightCacheKey {
    fn from_configs(
        boot_id_path: &Path,
        binary_config: &BinaryDiscoveryConfig,
        artifact_config: &ArtifactPreflightConfig,
    ) -> Result<Self, std::io::Error> {
        let rootfs = artifact_config.rootfs_image.as_ref().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "rootfs image not configured")
        })?;
        let manifest = manifest_path_for_rootfs(rootfs);
        let kernel = discover_kernel(artifact_config).map_err(std::io::Error::other)?;
        Ok(Self {
            boot_id: fs::read_to_string(boot_id_path)?.trim().to_owned(),
            expected_firecracker_version: binary_config.expected_firecracker_version.clone(),
            kernel_kind: artifact_config.kernel_kind.clone(),
            firecracker: FileIdentity::read(&binary_config.firecracker_bin)?,
            firecracker_seccomp_filter: FileIdentity::read(
                &binary_config.firecracker_seccomp_filter,
            )?,
            jailer: FileIdentity::read(&binary_config.jailer_bin)?,
            jailer_harden: FileIdentity::read(&binary_config.jailer_harden_bin)?,
            kernel: FileIdentity::read(&kernel)?,
            rootfs: FileIdentity::read(rootfs)?,
            manifest: FileIdentity::read(&manifest)?,
        })
    }

    fn digest(&self) -> String {
        let bytes = serde_json::to_vec(self).expect("preflight cache key serializes");
        hex::encode(Sha256::digest(bytes))
    }
}

impl FileIdentity {
    fn read(path: &Path) -> Result<Self, std::io::Error> {
        let metadata = fs::metadata(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            dev: metadata.dev(),
            ino: metadata.ino(),
            mtime_sec: metadata.mtime(),
            mtime_nsec: metadata.mtime_nsec(),
            size: metadata.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use m80_image_manifest::{ImageKind, KernelKind, Manifest, RootfsFormat};

    use super::{PreflightCache, PreflightSentinel, ENV_FORCE_PREFLIGHT};
    use crate::artifacts::ArtifactPreflightConfig;
    use crate::binary::BinaryDiscoveryConfig;

    const SHA256_EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    fn write_empty(path: &Path) {
        fs::write(path, b"").unwrap();
    }

    fn fixture_manifest(dir: &Path, kernel: &Path) -> (std::path::PathBuf, Manifest) {
        let rootfs = dir.join("rootfs.ext4");
        let daemon = dir.join("m80-guestd");
        write_empty(&rootfs);
        write_empty(&daemon);
        let manifest = Manifest::new(
            daemon,
            SHA256_EMPTY.to_string(),
            "v1.15.1".to_string(),
            9001,
            ImageKind::Minimal,
            kernel.to_path_buf(),
            SHA256_EMPTY.to_string(),
            KernelKind::Stock,
            None,
            rootfs.clone(),
            SHA256_EMPTY.to_string(),
            "M80_READY".to_string(),
            RootfsFormat::Ext4,
            None,
            None,
        );
        manifest
            .write(&crate::artifacts::manifest_path_for_rootfs(&rootfs))
            .unwrap();
        (rootfs, manifest)
    }

    fn fixture() -> (
        tempfile::TempDir,
        std::path::PathBuf,
        BinaryDiscoveryConfig,
        ArtifactPreflightConfig,
        Manifest,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let boot_id = dir.path().join("boot_id");
        fs::write(&boot_id, "boot-1\n").unwrap();
        let firecracker = dir.path().join("firecracker");
        let firecracker_seccomp_filter = dir.path().join("firecracker-seccomp-filter.json");
        let jailer = dir.path().join("jailer");
        let jailer_harden = dir.path().join("m80-jailer-harden");
        let kernel = dir.path().join("vmlinux-2027");
        write_empty(&firecracker);
        fs::write(&firecracker_seccomp_filter, b"{}").unwrap();
        write_empty(&jailer);
        write_empty(&jailer_harden);
        write_empty(&kernel);
        let (rootfs, manifest) = fixture_manifest(dir.path(), &kernel);
        let binary_config = BinaryDiscoveryConfig {
            firecracker_bin: firecracker,
            firecracker_seccomp_filter,
            jailer_bin: jailer,
            jailer_harden_bin: jailer_harden,
            expected_firecracker_version: Some("v1.15.1".to_owned()),
        };
        let artifact_config = ArtifactPreflightConfig {
            kernel_image: Some(kernel),
            artifact_dir: dir.path().to_path_buf(),
            rootfs_image: Some(rootfs),
            kernel_kind: None,
            run_root: dir.path().to_path_buf(),
            helper_search_path: None,
        };
        (dir, boot_id, binary_config, artifact_config, manifest)
    }

    #[test]
    fn corrupt_sentinel_is_ignored_and_rewritten() {
        let (dir, boot_id, binary_config, artifact_config, manifest) = fixture();
        let sentinel_dir = dir.path().join("sentinels");
        fs::create_dir(&sentinel_dir).unwrap();
        let cache = PreflightCache::load_from_dir(
            &sentinel_dir,
            &boot_id,
            &binary_config,
            &artifact_config,
        );
        fs::write(cache.path.as_ref().unwrap(), b"not-json").unwrap();

        let cache = PreflightCache::load_from_dir(
            &sentinel_dir,
            &boot_id,
            &binary_config,
            &artifact_config,
        );

        assert!(cache.hit().is_none());
        cache.store("v1.15.1", &manifest);

        let cache = PreflightCache::load_from_dir(
            &sentinel_dir,
            &boot_id,
            &binary_config,
            &artifact_config,
        );
        assert_eq!(cache.hit().unwrap().firecracker_version, "v1.15.1");
    }

    #[test]
    fn rootfs_mtime_change_invalidates_sentinel() {
        let (dir, boot_id, binary_config, artifact_config, manifest) = fixture();
        let sentinel_dir = dir.path().join("sentinels");
        fs::create_dir(&sentinel_dir).unwrap();
        let cache = PreflightCache::load_from_dir(
            &sentinel_dir,
            &boot_id,
            &binary_config,
            &artifact_config,
        );
        cache.store("v1.15.1", &manifest);

        fs::write(artifact_config.rootfs_image.as_ref().unwrap(), b"changed").unwrap();

        let cache = PreflightCache::load_from_dir(
            &sentinel_dir,
            &boot_id,
            &binary_config,
            &artifact_config,
        );
        assert!(cache.hit().is_none());
    }

    #[test]
    fn boot_id_change_invalidates_sentinel() {
        let (dir, boot_id, binary_config, artifact_config, manifest) = fixture();
        let sentinel_dir = dir.path().join("sentinels");
        fs::create_dir(&sentinel_dir).unwrap();
        let cache = PreflightCache::load_from_dir(
            &sentinel_dir,
            &boot_id,
            &binary_config,
            &artifact_config,
        );
        cache.store("v1.15.1", &manifest);

        fs::write(&boot_id, "boot-2\n").unwrap();

        let cache = PreflightCache::load_from_dir(
            &sentinel_dir,
            &boot_id,
            &binary_config,
            &artifact_config,
        );
        assert!(cache.hit().is_none());
    }

    #[test]
    fn matching_sentinel_reuses_cached_manifest() {
        let (dir, boot_id, binary_config, artifact_config, manifest) = fixture();
        let sentinel_dir = dir.path().join("sentinels");
        fs::create_dir(&sentinel_dir).unwrap();
        let cache = PreflightCache::load_from_dir(
            &sentinel_dir,
            &boot_id,
            &binary_config,
            &artifact_config,
        );
        cache.store("v1.15.1", &manifest);

        let cache = PreflightCache::load_from_dir(
            &sentinel_dir,
            &boot_id,
            &binary_config,
            &artifact_config,
        );

        let hit = cache.hit().unwrap();
        assert_eq!(hit.firecracker_version, "v1.15.1");
        assert_eq!(
            hit.manifest.output_rootfs_image,
            manifest.output_rootfs_image
        );
        let raw = fs::read(cache.path.as_ref().unwrap()).unwrap();
        serde_json::from_slice::<PreflightSentinel>(&raw).unwrap();
    }

    #[test]
    fn force_preflight_env_disables_cache_reads_and_writes() {
        let _lock = ENV_LOCK.lock().unwrap();
        let (_dir, _boot_id, binary_config, artifact_config, manifest) = fixture();
        let _force = EnvGuard::set_str(ENV_FORCE_PREFLIGHT, "1");

        let cache = PreflightCache::load(&binary_config, &artifact_config);
        cache.store("v1.15.1", &manifest);

        assert!(cache.key.is_none());
        assert!(cache.path.is_none());
        assert!(cache.hit().is_none());
    }

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct EnvGuard {
        key: &'static str,
        old: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set_str(key: &'static str, value: &str) -> Self {
            let old = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, old }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(old) = &self.old {
                std::env::set_var(self.key, old);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }
}
