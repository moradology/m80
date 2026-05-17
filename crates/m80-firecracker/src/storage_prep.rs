//! Phase-3 per-VM storage preparation.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use m80_image_store::{ImageArtifact, ImageKind, ImageStore, SharedImageRef};
use m80_storage::{Rootfs, Scratch};

use crate::diagnostics::phase_event;
use crate::error::{ConfigError, FcError};
use crate::layout::{
    pmem_layer_backing_dir, pmem_layer_backing_path, pmem_layer_jail_basename,
    preallocated_drive_slot_path, rootfs_overlay_path, scratch_image_path,
};
use crate::pmem::PmemSharing;
use crate::types::{ResolvedPmemBacking, SandboxConfig, StoragePrep};

/// Small placeholder backing size for pre-created Firecracker drive slots.
const PREALLOCATED_DRIVE_SLOT_BYTES: u64 = 1024 * 1024;
/// Firecracker v1.15.1 allocates virtio-pmem regions from the 512 GiB
/// `past_mmio64_memory` window after rounding the backing file up to 2 MiB.
const FIRECRACKER_V1_15_PMEM_WINDOW_BYTES: u64 = 512 * 1024 * 1024 * 1024;
const FIRECRACKER_PMEM_ALIGNMENT_BYTES: u64 = 2 * 1024 * 1024;

/// Phase 3: prepare the rootfs overlay; optionally create a scratch image for
/// the workspace.
pub(crate) fn phase_3_storage_prep(
    vm_id: &str,
    base_rootfs: &Path,
    config: &SandboxConfig,
    run_dir: &Path,
) -> Result<StoragePrep, FcError> {
    let overlay_dest = rootfs_overlay_path(run_dir);
    let t = Instant::now();
    let rootfs = Rootfs::prepare(
        base_rootfs,
        &overlay_dest,
        config.overlay_size_bytes,
        config.overlay_clone_mode,
    )?;
    phase_event("phase_3b_rootfs_prepare", vm_id, t.elapsed());

    let scratch = if let Some(workspace) = &config.workspace {
        let scratch_dest = scratch_image_path(run_dir);
        let size = Scratch::recommended_size_for_workspace(workspace)?;
        let t = Instant::now();
        let scratch = Scratch::create(workspace, &scratch_dest, size)?;
        phase_event("phase_3c_scratch_create", vm_id, t.elapsed());
        Some(scratch)
    } else {
        None
    };

    let preallocated_drive_slots =
        prepare_preallocated_drive_slots(run_dir, config.preallocated_drive_slots)?;
    let t = Instant::now();
    let pmem = phase_3b_resolve_pmem_backings(run_dir, vm_id, config)?;
    if !pmem.backings.is_empty() {
        phase_event("phase_3d_pmem_backings_prepare", vm_id, t.elapsed());
    }

    Ok(StoragePrep {
        rootfs,
        scratch,
        preallocated_drive_slots,
        pmem_backings: pmem.backings,
        shared_pmem_refs: pmem.shared_refs,
    })
}

#[derive(Debug, Default)]
pub(crate) struct PmemBackingPrep {
    pub(crate) backings: Vec<ResolvedPmemBacking>,
    pub(crate) shared_refs: Vec<SharedImageRef>,
}

pub(crate) fn phase_3b_resolve_pmem_backings(
    run_dir: &Path,
    vm_id: &str,
    config: &SandboxConfig,
) -> Result<PmemBackingPrep, FcError> {
    if config.pmem_layers.is_empty() {
        return Ok(PmemBackingPrep::default());
    }

    let store = ImageStore::open_default()?;
    phase_3b_resolve_pmem_backings_with_store(run_dir, vm_id, config, &store)
}

pub(crate) fn phase_3b_resolve_pmem_backings_with_store(
    run_dir: &Path,
    vm_id: &str,
    config: &SandboxConfig,
    store: &ImageStore,
) -> Result<PmemBackingPrep, FcError> {
    if config.pmem_layers.is_empty() {
        return Ok(PmemBackingPrep::default());
    }

    let mut resolved_images = Vec::with_capacity(config.pmem_layers.len());
    for layer in &config.pmem_layers {
        let store_digest = m80_image_store::ImageDigest::parse(layer.image().digest().as_str())
            .map_err(|source| {
                FcError::Config(crate::error::ConfigError::DigestInvalid {
                    reason: source.reason,
                })
            })?;
        let resolved = store.resolve_as(&store_digest, ImageKind::Erofs)?;
        let ImageArtifact::Erofs(image) = resolved else {
            return Err(FcError::InvalidState {
                expected: "erofs image artifact",
                actual: "non-erofs image artifact",
            });
        };
        validate_pmem_device_size(image.path(), image.size_bytes())?;
        if matches!(layer.sharing(), PmemSharing::Shared(_)) {
            validate_shared_erofs_layout(image.path())?;
        }
        resolved_images.push((store_digest, layer.sharing(), image.path().to_path_buf()));
    }

    if resolved_images
        .iter()
        .any(|(_, sharing, _)| matches!(sharing, PmemSharing::PerVm))
    {
        let backing_dir = pmem_layer_backing_dir(run_dir);
        std::fs::create_dir(&backing_dir).map_err(|source| FcError::PathIo {
            path: backing_dir.clone(),
            source,
        })?;
    }

    let mut backings = Vec::with_capacity(resolved_images.len());
    let mut shared_refs = Vec::new();
    for (slot, (digest, sharing, image_path)) in resolved_images.iter().enumerate() {
        let host_path = match sharing {
            PmemSharing::PerVm => {
                let dest = pmem_layer_backing_path(run_dir, slot);
                clone_pmem_backing(image_path, &dest)?;
                dest
            }
            PmemSharing::Shared(_) => {
                shared_refs.push(store.acquire_shared_ref(digest, vm_id)?);
                image_path.clone()
            }
        };
        backings.push(ResolvedPmemBacking {
            host_path,
            jail_basename: pmem_layer_jail_basename(slot),
            sharing: *sharing,
        });
    }
    Ok(PmemBackingPrep {
        backings,
        shared_refs,
    })
}

fn validate_shared_erofs_layout(path: &Path) -> Result<(), FcError> {
    let output = Command::new("dump.erofs")
        .arg("-S")
        .arg(path)
        .output()
        .map_err(|source| FcError::CommandSpawnFailed {
            command: "dump.erofs",
            source,
        })?;
    if !output.status.success() {
        return Err(FcError::CommandFailed {
            command: "dump.erofs",
            status: output.status,
            output: command_output_detail(&output.stdout, &output.stderr),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let compressed_files = parse_erofs_compressed_file_count(&stdout).ok_or_else(|| {
        FcError::Config(ConfigError::SharedPmemErofsLayoutProbeInvalid {
            path: path.to_path_buf(),
            reason: "dump.erofs -S output did not include compressed file count",
        })
    })?;
    if compressed_files > 0 {
        return Err(FcError::Config(ConfigError::SharedPmemCompressedErofs {
            path: path.to_path_buf(),
            compressed_files,
        }));
    }
    Ok(())
}

fn validate_pmem_device_size(path: &Path, size_bytes: u64) -> Result<(), FcError> {
    if size_bytes > FIRECRACKER_V1_15_PMEM_WINDOW_BYTES {
        return Err(FcError::Config(ConfigError::PmemImageTooLarge {
            path: path.to_path_buf(),
            got: size_bytes,
            max: FIRECRACKER_V1_15_PMEM_WINDOW_BYTES,
        }));
    }

    let rounded = round_pmem_backing_len(size_bytes);
    if rounded <= FIRECRACKER_V1_15_PMEM_WINDOW_BYTES {
        return Ok(());
    }
    Err(FcError::Config(ConfigError::PmemImageTooLarge {
        path: path.to_path_buf(),
        got: size_bytes,
        max: FIRECRACKER_V1_15_PMEM_WINDOW_BYTES,
    }))
}

fn round_pmem_backing_len(size_bytes: u64) -> u64 {
    let remainder = size_bytes % FIRECRACKER_PMEM_ALIGNMENT_BYTES;
    if remainder == 0 {
        return size_bytes;
    }
    size_bytes + FIRECRACKER_PMEM_ALIGNMENT_BYTES - remainder
}

fn parse_erofs_compressed_file_count(raw: &str) -> Option<u64> {
    raw.lines().find_map(|line| {
        line.trim_start()
            .strip_prefix("Filesystem compressed files:")
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|count| count.parse().ok())
    })
}

fn command_output_detail(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(stderr).trim().to_owned();
    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!(": {stdout}"),
        (true, false) => format!(": {stderr}"),
        (false, false) => format!(": {stdout}; {stderr}"),
    }
}

fn clone_pmem_backing(source: &Path, dest: &Path) -> Result<(), FcError> {
    let output = Command::new("cp")
        .arg("--reflink=auto")
        .arg("--sparse=always")
        .arg(source)
        .arg(dest)
        .output()
        .map_err(|source| FcError::CommandSpawnFailed {
            command: "cp",
            source,
        })?;
    if output.status.success() {
        return Ok(());
    }
    Err(FcError::CommandFailed {
        command: "cp",
        status: output.status,
        output: command_output_detail(&output.stdout, &output.stderr),
    })
}

// The create+truncate+write+set_len pattern appears in scratch.rs (StorageError)
// and rootfs.rs (custom StorageError variant) as well. All three sites differ in
// error wrapping and context; there is no useful shared helper across the crate
// boundary, so the duplication is acceptable.
fn prepare_preallocated_drive_slots(run_dir: &Path, count: u8) -> Result<Vec<PathBuf>, FcError> {
    let mut slots = Vec::with_capacity(usize::from(count));
    for slot in 0..count {
        let path = preallocated_drive_slot_path(run_dir, slot);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&path)
            .map_err(|source| FcError::PathIo {
                path: path.clone(),
                source,
            })?;
        file.set_len(PREALLOCATED_DRIVE_SLOT_BYTES)
            .map_err(|source| FcError::PathIo {
                path: path.clone(),
                source,
            })?;
        slots.push(path);
    }
    Ok(slots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt as _;

    use std::sync::{Arc, Barrier};
    use std::thread;

    use crate::{
        ErofsImageRef, GuestMountPath, ImageDigest, PmemLayer, PmemSharing, TrustDomainAck,
        TrustReason,
    };

    fn pmem_layer(digest: ImageDigest, name: &str) -> PmemLayer {
        PmemLayer::new(
            ErofsImageRef::from_digest(digest),
            PmemSharing::PerVm,
            GuestMountPath::parse(&format!("/opt/m80-layers/{name}")).unwrap(),
        )
    }

    fn shared_pmem_layer(digest: ImageDigest, name: &str) -> PmemLayer {
        PmemLayer::new(
            ErofsImageRef::from_digest(digest),
            PmemSharing::Shared(TrustDomainAck::new(TrustReason::SameOperator)),
            GuestMountPath::parse(&format!("/opt/m80-layers/{name}")).unwrap(),
        )
    }

    fn import_erofs(store: &ImageStore, source: &Path, bytes: &[u8]) -> ImageDigest {
        let source_dir = tempfile::tempdir_in(source.parent().unwrap()).unwrap();
        std::fs::write(source_dir.path().join("payload.bin"), bytes).unwrap();
        let output = Command::new("mkfs.erofs")
            .arg("--quiet")
            .arg("-T")
            .arg("0")
            .arg("--all-time")
            .arg("--all-root")
            .arg("--force-uid=0")
            .arg("--force-gid=0")
            .arg(source)
            .arg(source_dir.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "mkfs.erofs failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let digest = store.import_existing(source, ImageKind::Erofs).unwrap();
        ImageDigest::parse(digest.as_str()).unwrap()
    }

    fn build_minimal_erofs(store: &ImageStore, source_dir: &Path) -> ImageDigest {
        std::fs::create_dir_all(source_dir).unwrap();
        std::fs::write(source_dir.join("payload.bin"), vec![b'a'; 1024 * 1024]).unwrap();
        let digest = store
            .build_minimal_test_image(source_dir, ImageKind::Erofs)
            .unwrap();
        ImageDigest::parse(digest.as_str()).unwrap()
    }

    fn import_compressed_erofs(
        store: &ImageStore,
        source_dir: &Path,
        output: &Path,
    ) -> ImageDigest {
        std::fs::create_dir_all(source_dir).unwrap();
        std::fs::write(source_dir.join("payload.bin"), vec![b'z'; 1024 * 1024]).unwrap();
        let output_status = Command::new("mkfs.erofs")
            .arg("--quiet")
            .arg("-zlz4hc,level=9")
            .arg(output)
            .arg(source_dir)
            .output()
            .unwrap();
        assert!(
            output_status.status.success(),
            "mkfs.erofs failed: {}",
            String::from_utf8_lossy(&output_status.stderr)
        );
        let digest = store.import_existing(output, ImageKind::Erofs).unwrap();
        ImageDigest::parse(digest.as_str()).unwrap()
    }

    fn file_identity(path: &Path) -> (u64, u64) {
        let metadata = std::fs::metadata(path).unwrap();
        (metadata.dev(), metadata.ino())
    }
    use crate::layout::preallocated_drive_slot_filename;

    #[test]
    fn preallocated_drive_slot_prep_returns_canonical_paths() {
        let dir = tempfile::tempdir().unwrap();

        let slots = prepare_preallocated_drive_slots(dir.path(), 2).unwrap();

        assert_eq!(
            slots,
            vec![
                dir.path().join(preallocated_drive_slot_filename(0)),
                dir.path().join(preallocated_drive_slot_filename(1)),
            ]
        );
    }

    #[test]
    fn preallocated_drive_slot_prep_creates_placeholder_sized_files() {
        let dir = tempfile::tempdir().unwrap();

        let slots = prepare_preallocated_drive_slots(dir.path(), 2).unwrap();

        for slot in slots {
            assert_eq!(
                std::fs::metadata(slot).unwrap().len(),
                PREALLOCATED_DRIVE_SLOT_BYTES
            );
        }
    }

    #[test]
    fn pmem_device_size_validation_accepts_firecracker_window_edge() {
        let path = Path::new("/store/toolchain.erofs");

        validate_pmem_device_size(path, FIRECRACKER_V1_15_PMEM_WINDOW_BYTES).unwrap();
        validate_pmem_device_size(
            path,
            FIRECRACKER_V1_15_PMEM_WINDOW_BYTES - FIRECRACKER_PMEM_ALIGNMENT_BYTES + 1,
        )
        .unwrap();
    }

    #[test]
    fn pmem_device_size_validation_rejects_over_firecracker_window() {
        let path = Path::new("/store/toolchain.erofs");
        let got = FIRECRACKER_V1_15_PMEM_WINDOW_BYTES + 1;

        let err = validate_pmem_device_size(path, got).unwrap_err();

        assert!(
            matches!(
                err,
                FcError::Config(ConfigError::PmemImageTooLarge {
                    ref path,
                    got: rejected,
                    max: FIRECRACKER_V1_15_PMEM_WINDOW_BYTES,
                }) if path == Path::new("/store/toolchain.erofs") && rejected == got
            ),
            "got {err:?}"
        );
    }

    #[test]
    fn pmem_backing_prep_clones_per_vm_files_with_distinct_inodes() {
        let run_dir = tempfile::tempdir().unwrap();
        let store_root = tempfile::tempdir().unwrap();
        let store = ImageStore::open(store_root.path()).unwrap();
        let source = store_root.path().join("toolchain.erofs");
        let digest = import_erofs(&store, &source, b"erofs bytes");
        let source_len = source.metadata().unwrap().len();
        let mut config = SandboxConfig::default();
        config.pmem_layers = vec![
            pmem_layer(digest.clone(), "toolchain-a"),
            pmem_layer(digest, "toolchain-b"),
        ];

        let prep =
            phase_3b_resolve_pmem_backings_with_store(run_dir.path(), "vm-per-vm", &config, &store)
                .unwrap();
        let backings = prep.backings;

        assert_eq!(backings.len(), 2);
        assert_eq!(backings[0].jail_basename, "pmem.0.img");
        assert_eq!(backings[1].jail_basename, "pmem.1.img");
        assert_eq!(backings[0].sharing, PmemSharing::PerVm);
        assert_eq!(backings[1].sharing, PmemSharing::PerVm);
        assert_eq!(
            backings
                .iter()
                .map(|backing| backing.host_path.clone())
                .collect::<Vec<_>>(),
            vec![
                run_dir.path().join("pmem/0.img"),
                run_dir.path().join("pmem/1.img"),
            ]
        );
        let first = std::fs::metadata(&backings[0].host_path).unwrap();
        let second = std::fs::metadata(&backings[1].host_path).unwrap();
        assert_eq!(first.len(), source_len);
        assert_eq!(second.len(), source_len);
        assert_ne!(first.ino(), second.ino());
    }

    #[test]
    fn shared_pmem_backing_reuses_store_inode_without_clone() {
        let first_run_dir = tempfile::tempdir().unwrap();
        let second_run_dir = tempfile::tempdir().unwrap();
        let store_root = tempfile::tempdir().unwrap();
        let store = ImageStore::open(store_root.path()).unwrap();
        let source = tempfile::tempdir().unwrap();
        let digest = build_minimal_erofs(&store, source.path());
        let mut config = SandboxConfig::default();
        config.pmem_layers = vec![shared_pmem_layer(digest, "toolchain")];

        let first = phase_3b_resolve_pmem_backings_with_store(
            first_run_dir.path(),
            "vm-shared-a",
            &config,
            &store,
        )
        .unwrap();
        let second = phase_3b_resolve_pmem_backings_with_store(
            second_run_dir.path(),
            "vm-shared-b",
            &config,
            &store,
        )
        .unwrap();
        assert_eq!(first.shared_refs.len(), 1);
        assert_eq!(second.shared_refs.len(), 1);
        let first_refs = first.shared_refs;
        let second_refs = second.shared_refs;
        let first = first.backings;
        let second = second.backings;

        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_eq!(first[0].jail_basename, "pmem.0.img");
        assert_eq!(second[0].jail_basename, "pmem.0.img");
        assert!(matches!(first[0].sharing, PmemSharing::Shared(_)));
        assert!(matches!(second[0].sharing, PmemSharing::Shared(_)));
        assert_eq!(first[0].host_path, second[0].host_path);
        assert_eq!(
            file_identity(&first[0].host_path),
            file_identity(&second[0].host_path)
        );
        assert!(
            !first_run_dir.path().join("pmem").exists(),
            "shared backing must not create a per-VM pmem clone directory"
        );
        assert!(
            !second_run_dir.path().join("pmem").exists(),
            "shared backing must not create a per-VM pmem clone directory"
        );
        for shared_ref in first_refs.into_iter().chain(second_refs) {
            shared_ref.release().unwrap();
        }
    }

    #[test]
    fn shared_pmem_backing_is_idempotent_under_concurrent_attach() {
        let first_run_dir = tempfile::tempdir().unwrap();
        let second_run_dir = tempfile::tempdir().unwrap();
        let store_root = tempfile::tempdir().unwrap();
        let store = ImageStore::open(store_root.path()).unwrap();
        let source = tempfile::tempdir().unwrap();
        let digest = build_minimal_erofs(&store, source.path());
        let mut config = SandboxConfig::default();
        config.pmem_layers = vec![shared_pmem_layer(digest, "toolchain")];
        let barrier = Arc::new(Barrier::new(2));

        let first_store = store.clone();
        let first_config = config.clone();
        let first_path = first_run_dir.path().to_path_buf();
        let first_barrier = Arc::clone(&barrier);
        let first = thread::spawn(move || {
            first_barrier.wait();
            phase_3b_resolve_pmem_backings_with_store(
                &first_path,
                "vm-concurrent-a",
                &first_config,
                &first_store,
            )
        });

        let second_store = store;
        let second_config = config;
        let second_path = second_run_dir.path().to_path_buf();
        let second_barrier = Arc::clone(&barrier);
        let second = thread::spawn(move || {
            second_barrier.wait();
            phase_3b_resolve_pmem_backings_with_store(
                &second_path,
                "vm-concurrent-b",
                &second_config,
                &second_store,
            )
        });

        let first = first.join().unwrap().unwrap();
        let second = second.join().unwrap().unwrap();
        assert_eq!(first.shared_refs.len(), 1);
        assert_eq!(second.shared_refs.len(), 1);
        let first_refs = first.shared_refs;
        let second_refs = second.shared_refs;
        let first = first.backings;
        let second = second.backings;

        assert_eq!(first[0].host_path, second[0].host_path);
        assert_eq!(
            file_identity(&first[0].host_path),
            file_identity(&second[0].host_path)
        );
        for shared_ref in first_refs.into_iter().chain(second_refs) {
            shared_ref.release().unwrap();
        }
    }

    #[test]
    fn mixed_pmem_sharing_keeps_per_vm_clone_and_shared_store_inode() {
        let run_dir = tempfile::tempdir().unwrap();
        let store_root = tempfile::tempdir().unwrap();
        let store = ImageStore::open(store_root.path()).unwrap();
        let per_vm_source = store_root.path().join("per-vm.erofs");
        let shared_source = tempfile::tempdir().unwrap();
        let per_vm_digest = import_erofs(&store, &per_vm_source, b"per vm bytes");
        let shared_digest = build_minimal_erofs(&store, shared_source.path());
        let mut config = SandboxConfig::default();
        config.pmem_layers = vec![
            pmem_layer(per_vm_digest, "per-vm"),
            shared_pmem_layer(shared_digest, "shared"),
        ];

        let prep =
            phase_3b_resolve_pmem_backings_with_store(run_dir.path(), "vm-mixed", &config, &store)
                .unwrap();
        assert_eq!(prep.shared_refs.len(), 1);
        let shared_refs = prep.shared_refs;
        let backings = prep.backings;

        assert_eq!(backings.len(), 2);
        assert_eq!(backings[0].sharing, PmemSharing::PerVm);
        assert!(matches!(backings[1].sharing, PmemSharing::Shared(_)));
        assert_eq!(backings[0].host_path, run_dir.path().join("pmem/0.img"));
        assert_ne!(backings[1].host_path, run_dir.path().join("pmem/1.img"));
        assert!(backings[0].host_path.exists());
        assert!(backings[1].host_path.exists());
        assert_ne!(
            file_identity(&backings[0].host_path),
            file_identity(&backings[1].host_path)
        );
        for shared_ref in shared_refs {
            shared_ref.release().unwrap();
        }
    }

    #[test]
    fn shared_pmem_rejects_compressed_erofs_before_markers_or_jail_materialization() {
        let run_dir = tempfile::tempdir().unwrap();
        let store_root = tempfile::tempdir().unwrap();
        let store = ImageStore::open(store_root.path()).unwrap();
        let source = tempfile::tempdir().unwrap();
        let compressed = store_root.path().join("compressed.erofs");
        let digest = import_compressed_erofs(&store, source.path(), &compressed);
        let store_digest = m80_image_store::ImageDigest::parse(digest.as_str()).unwrap();
        let mut config = SandboxConfig::default();
        config.pmem_layers = vec![shared_pmem_layer(digest, "compressed")];

        let err = phase_3b_resolve_pmem_backings_with_store(
            run_dir.path(),
            "vm-compressed-shared",
            &config,
            &store,
        )
        .unwrap_err();

        assert!(
            matches!(
                err,
                FcError::Config(ConfigError::SharedPmemCompressedErofs {
                    compressed_files,
                    ..
                }) if compressed_files > 0
            ),
            "got {err:?}"
        );
        assert!(
            !run_dir.path().join("pmem").exists(),
            "compressed Shared rejection must happen before jail backing materialization"
        );
        assert_eq!(
            store.shared_ref_count(&store_digest).unwrap(),
            0,
            "compressed Shared rejection must happen before active-use marker creation"
        );
    }

    #[test]
    fn missing_pmem_image_fails_before_backing_dir_created() {
        let run_dir = tempfile::tempdir().unwrap();
        let store_root = tempfile::tempdir().unwrap();
        let store = ImageStore::open(store_root.path()).unwrap();
        let digest =
            ImageDigest::parse("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff")
                .unwrap();
        let mut config = SandboxConfig::default();
        config.pmem_layers = vec![pmem_layer(digest, "missing")];

        let err = phase_3b_resolve_pmem_backings_with_store(
            run_dir.path(),
            "vm-missing",
            &config,
            &store,
        )
        .unwrap_err();

        assert!(
            matches!(
                err,
                FcError::ImageStore(m80_image_store::StoreError::NotFound { .. })
            ),
            "got {err:?}"
        );
        assert!(
            !run_dir.path().join("pmem").exists(),
            "missing image must fail before creating pmem backing dir"
        );
    }

    #[test]
    fn missing_shared_pmem_image_fails_before_backing_dir_created() {
        let run_dir = tempfile::tempdir().unwrap();
        let store_root = tempfile::tempdir().unwrap();
        let store = ImageStore::open(store_root.path()).unwrap();
        let digest =
            ImageDigest::parse("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff")
                .unwrap();
        let mut config = SandboxConfig::default();
        config.pmem_layers = vec![shared_pmem_layer(digest, "missing")];

        let err = phase_3b_resolve_pmem_backings_with_store(
            run_dir.path(),
            "vm-missing-shared",
            &config,
            &store,
        )
        .unwrap_err();

        assert!(
            matches!(
                err,
                FcError::ImageStore(m80_image_store::StoreError::NotFound { .. })
            ),
            "got {err:?}"
        );
        assert!(
            !run_dir.path().join("pmem").exists(),
            "missing shared image must fail before creating pmem backing dir"
        );
    }
}
