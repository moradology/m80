//! Phase-3 per-VM storage preparation.

use std::path::{Path, PathBuf};
use std::time::Instant;

use m80_storage::{Rootfs, Scratch};

use crate::diagnostics::phase_event;
use crate::error::FcError;
use crate::layout::{preallocated_drive_slot_path, rootfs_overlay_path, scratch_image_path};
use crate::types::{SandboxConfig, StoragePrep};

/// Default scratch size: 64 MiB.
const SCRATCH_DEFAULT_BYTES: u64 = 64 * 1024 * 1024;

/// Small placeholder backing size for pre-created Firecracker drive slots.
const PREALLOCATED_DRIVE_SLOT_BYTES: u64 = 1024 * 1024;

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
    let rootfs = Rootfs::prepare(base_rootfs, &overlay_dest, config.overlay_size_bytes)?;
    phase_event("phase_3b_rootfs_prepare", vm_id, t.elapsed());

    let scratch = if let Some(workspace) = &config.workspace {
        let scratch_dest = scratch_image_path(run_dir);
        let t = Instant::now();
        let scratch = Scratch::create(workspace, &scratch_dest, SCRATCH_DEFAULT_BYTES)?;
        phase_event("phase_3c_scratch_create", vm_id, t.elapsed());
        Some(scratch)
    } else {
        None
    };

    let preallocated_drive_slots =
        prepare_preallocated_drive_slots(run_dir, config.preallocated_drive_slots)?;

    Ok(StoragePrep {
        rootfs,
        scratch,
        preallocated_drive_slots,
    })
}

fn prepare_preallocated_drive_slots(run_dir: &Path, count: u8) -> Result<Vec<PathBuf>, FcError> {
    let mut slots = Vec::with_capacity(usize::from(count));
    for slot in 0..count {
        let path = preallocated_drive_slot_path(run_dir, slot);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&path)?;
        file.set_len(PREALLOCATED_DRIVE_SLOT_BYTES)?;
        slots.push(path);
    }
    Ok(slots)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preallocated_drive_slot_prep_creates_sparse_placeholders() {
        let dir = tempfile::tempdir().unwrap();

        let slots = prepare_preallocated_drive_slots(dir.path(), 2).unwrap();

        assert_eq!(
            slots,
            vec![
                dir.path().join("hotplug-slot-0.raw"),
                dir.path().join("hotplug-slot-1.raw")
            ]
        );
        for slot in slots {
            assert_eq!(
                std::fs::metadata(slot).unwrap().len(),
                PREALLOCATED_DRIVE_SLOT_BYTES
            );
        }
    }
}
