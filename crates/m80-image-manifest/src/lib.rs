//! Schema and verification for m80 provenance manifests.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: beads `m80-sz1.3`, `m80-sz1.4` (`br show m80-sz1.3`).

#![deny(missing_docs)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Manifest schema version.
///
/// `2` — added `image_kind` discriminator and made systemd-related fields
/// `Option<>` so a `Minimal` image (PID-1 m80-guestd, no systemd) can
/// emit a manifest without lying about which artifacts exist.
///
/// `3` — added `kernel_kind: KernelKind` (defaults to `Stock` via
/// `#[serde(default)]` so existing v2 manifests must be rebuilt for v3).
///
/// `4` — removed the systemd startup artifacts from the manifest. Both
/// image kinds now boot m80-guestd as PID 1; `Ubuntu` records only its
/// source rootfs provenance in addition to the common artifacts.
///
/// `5` — added `rootfs_format: RootfsFormat` so host launch planning and
/// PID-1 guest setup agree on whether the read-only base drive is ext4 or
/// erofs.
///
/// No 1↔2↔3↔4↔5 conversion code: per CLAUDE.md, future versions are new code,
/// not migrations. Existing older images must be rebuilt.
pub const SCHEMA_VERSION: u32 = 5;

/// Schema version for `host-binaries.manifest.json`.
pub const HOST_BINARIES_SCHEMA_VERSION: u32 = 1;

/// Human-readable audit reason recorded in m80-built images that do not bake
/// an outbound network posture into the image itself.
pub const DEFAULT_NO_EGRESS_REASON: &str =
    "no-egress: image is network-neutral; outbound access requires runtime policy";

/// Which kernel was used to boot this image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum KernelKind {
    /// Upstream Firecracker CI kernel (downloaded from the S3 bucket).
    #[default]
    Stock,
    /// Purpose-built stripped kernel (built via m80-ci9i.2 pipeline).
    Stripped,
}

/// Which startup model the image was built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum ImageKind {
    /// Ubuntu userland rootfs from the Firecracker CI squashfs. m80-guestd
    /// still runs as PID 1 so the base+overlay+workspace drive contract is
    /// identical to [`ImageKind::Minimal`].
    Ubuntu,
    /// Minimal rootfs with busybox userland. m80-guestd runs as PID 1
    /// (`init=/m80-guestd`).
    Minimal,
}

/// Filesystem format of the read-only base rootfs drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum RootfsFormat {
    /// ext4 base rootfs.
    Ext4,
    /// erofs base rootfs.
    Erofs,
}

/// Host-side binary names covered by `host-binaries.manifest.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum HostBinaryName {
    /// Firecracker VMM executable.
    Firecracker,
    /// Official Firecracker jailer executable.
    Jailer,
    /// Installed m80 CLI executable.
    M80,
    /// Installed m80-cli release artifact.
    M80Cli,
    /// m80 hardening wrapper that execs the official jailer.
    M80JailerHarden,
}

impl HostBinaryName {
    /// Stable manifest spelling for diagnostics.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Firecracker => "firecracker",
            Self::Jailer => "jailer",
            Self::M80 => "m80",
            Self::M80Cli => "m80_cli",
            Self::M80JailerHarden => "m80_jailer_harden",
        }
    }
}

/// One host-side TCB binary recorded at install time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostBinaryEntry {
    /// Logical binary name.
    pub name: HostBinaryName,
    /// Absolute installed path.
    pub path: PathBuf,
    /// sha256 hex digest of the installed binary bytes.
    pub sha256: String,
}

/// Install-time manifest for host-side TCB binaries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostBinariesManifest {
    /// Host TCB binaries covered by this manifest.
    pub binaries: Vec<HostBinaryEntry>,
    /// Always [`HOST_BINARIES_SCHEMA_VERSION`].
    schema_version: u32,
}

/// Probes only `schema_version` for `host-binaries.manifest.json`.
#[derive(Deserialize)]
struct HostBinariesSchemaVersionProbe {
    schema_version: u32,
}

impl HostBinariesManifest {
    /// Construct a host-binaries manifest with the current schema version.
    #[must_use]
    pub fn new(binaries: Vec<HostBinaryEntry>) -> Self {
        Self {
            binaries,
            schema_version: HOST_BINARIES_SCHEMA_VERSION,
        }
    }

    /// Returns the host-binaries schema version.
    #[must_use]
    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Parse a host-binaries manifest from raw bytes.
    pub fn from_bytes(raw: &[u8]) -> Result<Self, ManifestError> {
        let probe: HostBinariesSchemaVersionProbe = serde_json::from_slice(raw)?;
        if probe.schema_version != HOST_BINARIES_SCHEMA_VERSION {
            return Err(ManifestError::UnsupportedHostBinariesSchemaVersion(
                probe.schema_version,
            ));
        }
        Ok(serde_json::from_slice(raw)?)
    }

    /// Read and structurally validate a host-binaries manifest.
    pub fn read(path: &Path) -> Result<Self, ManifestError> {
        let raw = std::fs::read(path).map_err(|source| ManifestError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_bytes(&raw)
    }

    /// Write this host-binaries manifest as pretty JSON with mode 0644.
    pub fn write(&self, path: &Path) -> Result<(), ManifestError> {
        if self.schema_version != HOST_BINARIES_SCHEMA_VERSION {
            return Err(ManifestError::UnsupportedHostBinariesSchemaVersion(
                self.schema_version,
            ));
        }
        let mut json = serde_json::to_string_pretty(self)?;
        json.push('\n');
        std::fs::write(path, json.as_bytes()).map_err(|source| ManifestError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o644);
            std::fs::set_permissions(path, perms).map_err(|source| ManifestError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        }
        Ok(())
    }
}

/// The provenance manifest for a built guest image. Single source of truth
/// shared by `m80-image-build` (writer) and `m80-preflight` (reader/verifier)
/// so the schema cannot drift. Field declaration order is alphabetical so
/// JSON serialization is byte-stable without a canonicalization pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Host-side audit copy of the daemon binary that was installed into
    /// the image. `Manifest::verify` recomputes the sha256 of this file at
    /// preflight time without loop-mounting the rootfs. The in-VM
    /// destination is a build constant, not stored here.
    pub daemon_binary_path: PathBuf,
    /// sha256 hex digest of the daemon binary.
    pub daemon_binary_sha256: String,
    /// Firecracker version this image is pinned to (e.g., `"v1.15.1"`).
    pub expected_firecracker_version: String,
    /// Vsock port the in-VM daemon listens on.
    pub guest_port: u32,
    /// Which startup model this image was built for. Required.
    pub image_kind: ImageKind,
    /// Absolute path of the kernel image at build time.
    pub kernel_image: PathBuf,
    /// sha256 hex digest of the kernel image bytes.
    pub kernel_image_sha256: String,
    /// Which kernel was used to boot this image.
    pub kernel_kind: KernelKind,
    /// Free-text reason recorded when the image is built without egress
    /// configured.
    pub no_egress_reason: Option<String>,
    /// Absolute path of the built rootfs (the one Firecracker mounts).
    pub output_rootfs_image: PathBuf,
    /// sha256 hex digest of the built rootfs bytes.
    pub output_rootfs_sha256: String,
    /// Serial-console marker the guest emits when the daemon is ready.
    pub ready_marker: String,
    /// Filesystem format of the read-only base rootfs drive.
    pub rootfs_format: RootfsFormat,
    /// Always [`SCHEMA_VERSION`]. Private so callers cannot supply an
    /// arbitrary version; use [`Manifest::new`] to construct and
    /// [`Manifest::schema_version`] to read.
    schema_version: u32,
    /// Absolute path of the source rootfs (squashfs or upstream ext4).
    /// `None` for `Minimal` images (built from scratch with no upstream).
    pub source_rootfs_image: Option<PathBuf>,
    /// sha256 hex digest of the source rootfs bytes. `None` for
    /// `Minimal` images.
    pub source_rootfs_sha256: Option<String>,
}

/// Probes only `schema_version` so a future-version manifest reports
/// `UnsupportedSchemaVersion` instead of leaking the unrelated
/// `Json("unknown field …")` from `deny_unknown_fields`.
#[derive(Deserialize)]
struct SchemaVersionProbe {
    schema_version: u32,
}

impl Manifest {
    /// Construct a new [`Manifest`] with `schema_version` automatically set
    /// to [`SCHEMA_VERSION`]. This is the only construction path; the field
    /// is private so callers cannot supply an arbitrary version.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        daemon_binary_path: PathBuf,
        daemon_binary_sha256: String,
        expected_firecracker_version: String,
        guest_port: u32,
        image_kind: ImageKind,
        kernel_image: PathBuf,
        kernel_image_sha256: String,
        kernel_kind: KernelKind,
        no_egress_reason: Option<String>,
        output_rootfs_image: PathBuf,
        output_rootfs_sha256: String,
        ready_marker: String,
        rootfs_format: RootfsFormat,
        source_rootfs_image: Option<PathBuf>,
        source_rootfs_sha256: Option<String>,
    ) -> Manifest {
        Manifest {
            daemon_binary_path,
            daemon_binary_sha256,
            expected_firecracker_version,
            guest_port,
            image_kind,
            kernel_image,
            kernel_image_sha256,
            kernel_kind,
            no_egress_reason,
            output_rootfs_image,
            output_rootfs_sha256,
            ready_marker,
            rootfs_format,
            schema_version: SCHEMA_VERSION,
            source_rootfs_image,
            source_rootfs_sha256,
        }
    }

    /// Returns the manifest's schema version; always equal to [`SCHEMA_VERSION`].
    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Parse and structurally validate a manifest from raw bytes. Probes
    /// `schema_version` before the full parse so future-version payloads
    /// report `UnsupportedSchemaVersion` instead of leaking
    /// `Json("unknown field …")` from `deny_unknown_fields`.
    /// Does NOT verify sha256s — call [`Manifest::verify`] for that.
    pub fn from_bytes(raw: &[u8]) -> Result<Manifest, ManifestError> {
        let probe: SchemaVersionProbe = serde_json::from_slice(raw)?;
        if probe.schema_version != SCHEMA_VERSION {
            return Err(ManifestError::UnsupportedSchemaVersion(
                probe.schema_version,
            ));
        }
        let manifest: Manifest = serde_json::from_slice(raw)?;
        manifest.check_kind_invariants()?;
        Ok(manifest)
    }

    /// Read and structurally validate a manifest. Probes `schema_version`
    /// before the full parse so future-version files report cleanly.
    /// Does NOT verify sha256s — call [`Manifest::verify`] for that.
    pub fn read(path: &Path) -> Result<Manifest, ManifestError> {
        let raw = std::fs::read(path).map_err(|source| ManifestError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Manifest::from_bytes(&raw)
    }

    /// Write this manifest to `path` as pretty-printed JSON with a trailing
    /// newline and mode 0644 on Unix.
    ///
    /// Field order in the output is alphabetical (struct field order). The
    /// caller is responsible for the parent directory existing.
    pub fn write(&self, path: &Path) -> Result<(), ManifestError> {
        self.check_kind_invariants()?;
        let mut json = serde_json::to_string_pretty(self)?;
        json.push('\n');
        std::fs::write(path, json.as_bytes()).map_err(|source| ManifestError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o644);
            std::fs::set_permissions(path, perms).map_err(|source| ManifestError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        }
        Ok(())
    }

    /// Recompute every recorded sha256 from the on-disk artifact and compare
    /// against the manifest. Any mismatch is fatal; there is no "warn and
    /// continue."
    ///
    /// Paths recorded in the manifest are used as-is when absolute, or
    /// resolved relative to `root` when relative. Fields that are `None`
    /// for the manifest's `image_kind` are skipped (e.g., `Minimal` has
    /// no systemd unit to verify).
    pub fn verify(&self, root: &Path) -> Result<(), ManifestError> {
        self.check_kind_invariants()?;
        let resolve = |p: &Path| -> PathBuf {
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                root.join(p)
            }
        };

        check_sha256(
            "kernel_image",
            &resolve(&self.kernel_image),
            &self.kernel_image_sha256,
        )?;
        if let (Some(src_path), Some(src_sha)) =
            (&self.source_rootfs_image, &self.source_rootfs_sha256)
        {
            check_sha256("source_rootfs_image", &resolve(src_path), src_sha)?;
        }
        check_sha256(
            "output_rootfs_image",
            &resolve(&self.output_rootfs_image),
            &self.output_rootfs_sha256,
        )?;
        check_sha256(
            "daemon_binary_path",
            &resolve(&self.daemon_binary_path),
            &self.daemon_binary_sha256,
        )?;
        Ok(())
    }

    /// Enforce `image_kind`'s invariants on Optional fields:
    /// - `Ubuntu` requires source-rootfs fields populated.
    /// - `Minimal` requires source-rootfs fields `None`.
    fn check_kind_invariants(&self) -> Result<(), ManifestError> {
        match self.image_kind {
            ImageKind::Ubuntu => {
                let pairs: &[(&str, bool)] = &[
                    ("source_rootfs_image", self.source_rootfs_image.is_some()),
                    ("source_rootfs_sha256", self.source_rootfs_sha256.is_some()),
                ];
                for (name, present) in pairs {
                    if !present {
                        return Err(ManifestError::InconsistentKind {
                            kind: ImageKind::Ubuntu,
                            field: (*name).to_owned(),
                            expected: "Some(_)",
                        });
                    }
                }
            }
            ImageKind::Minimal => {
                let pairs: &[(&str, bool)] = &[
                    ("source_rootfs_image", self.source_rootfs_image.is_some()),
                    ("source_rootfs_sha256", self.source_rootfs_sha256.is_some()),
                ];
                for (name, present) in pairs {
                    if *present {
                        return Err(ManifestError::InconsistentKind {
                            kind: ImageKind::Minimal,
                            field: (*name).to_owned(),
                            expected: "None",
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

fn check_sha256(field: &str, path: &Path, expected: &str) -> Result<(), ManifestError> {
    use std::io::Read;
    // Stream the file through Sha256 in 64 KiB chunks so verifying a
    // multi-GiB rootfs doesn't allocate the whole file on the heap.
    let mut file = std::fs::File::open(path).map_err(|source| ManifestError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut n = file.read(&mut buf).map_err(|source| ManifestError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    while n != 0 {
        hasher.update(&buf[..n]);
        n = file.read(&mut buf).map_err(|source| ManifestError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    }
    let actual = hex::encode(hasher.finalize());
    if expected != actual {
        return Err(ManifestError::Sha256Mismatch {
            field: field.to_owned(),
            expected: expected.to_owned(),
            actual,
        });
    }
    Ok(())
}

/// Errors surfaced by manifest read / write / verify.
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    /// `schema_version` was not [`SCHEMA_VERSION`]. Reported via the
    /// `SchemaVersionProbe` partial parse before `deny_unknown_fields`
    /// errors fire.
    #[error("unsupported manifest schema version: got {0}, expected {SCHEMA_VERSION}")]
    UnsupportedSchemaVersion(u32),
    /// `host-binaries.manifest.json` carried an unsupported schema version.
    #[error(
        "unsupported host-binaries manifest schema version: got {0}, expected {HOST_BINARIES_SCHEMA_VERSION}"
    )]
    UnsupportedHostBinariesSchemaVersion(u32),
    /// A recomputed sha256 did not match the recorded value.
    #[error("sha256 mismatch on {field}: expected {expected}, got {actual}")]
    Sha256Mismatch {
        /// Manifest field whose hash failed (e.g., `"kernel_image"`).
        field: String,
        /// Recorded hex digest.
        expected: String,
        /// Recomputed hex digest.
        actual: String,
    },
    /// Optional field's presence does not match the manifest's `image_kind`.
    /// `Ubuntu` requires source-rootfs Options populated; `Minimal` requires
    /// them `None`.
    #[error(
        "manifest field {field} is inconsistent with image_kind={kind:?} (expected {expected})"
    )]
    InconsistentKind {
        /// The manifest's declared kind.
        kind: ImageKind,
        /// Which field tripped the invariant.
        field: String,
        /// Whether the field should be `Some(_)` or `None`.
        expected: &'static str,
    },
    /// I/O failure on a manifest read or artifact read; carries the path so
    /// the caller doesn't have to guess which file failed.
    #[error("i/o on {}: {source}", path.display())]
    Io {
        /// File the I/O was attempted against.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// JSON encode/decode failure (malformed JSON, missing required field,
    /// or unknown field rejected by `deny_unknown_fields`).
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}
