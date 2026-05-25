//! Pure path helpers for the per-VM run-root layout.

use std::path::{Path, PathBuf};

/// AF_UNIX `sun_path` is 108 bytes including the null terminator; usable path
/// bytes therefore max out at 107. Sockets bound at the kernel cap or above
/// fail with `EINVAL` ("AF_UNIX path too long") at `bind()`/`connect()` time.
pub(crate) const SUN_PATH_BUDGET: usize = 107;

/// Firecracker REST API socket filename inside the jail root.
pub(crate) const FIRECRACKER_API_SOCKET: &str = "firecracker.sock";

/// Host-side vsock muxer socket filename inside the jail root.
pub(crate) const VSOCK_SOCKET: &str = "vsock.sock";

/// Per-VM writable root filesystem overlay filename in the run directory.
pub const ROOTFS_OVERLAY_IMAGE: &str = "rootfs.overlay.ext4";

/// Per-VM writable workspace scratch image filename in the run directory.
pub(crate) const SCRATCH_IMAGE: &str = "scratch.ext4";

pub(crate) const PREALLOCATED_DRIVE_SLOT_PREFIX: &str = "hotplug-slot-";
pub(crate) const PMEM_BACKING_DIR: &str = "pmem";
pub(crate) const PMEM_JAIL_PREFIX: &str = "pmem.";

/// Per-VM console and process stderr log filename in the run directory.
pub const CONSOLE_LOG: &str = "console.log";

/// Firecracker native structured logger filename inside the jail root.
pub const FIRECRACKER_LOG: &str = "firecracker.log";

/// Firecracker native JSON metrics filename inside the jail root.
pub const FIRECRACKER_METRICS: &str = "firecracker-metrics.jsonl";

/// Boot artifact identity filename in the run directory.
pub const BOOT_IDENTITY_FILE: &str = "boot-identity.json";

/// Compute the per-VM run directory under the backend run root.
#[must_use]
pub fn run_dir_path(run_root: &Path, vm_id: &str) -> PathBuf {
    run_root.join(vm_id)
}

/// Compute the on-host length of the Firecracker REST API socket path.
///
/// Mirrors [`run_dir_path`] + [`m80_jailer::jail_root_path`] +
/// [`FIRECRACKER_API_SOCKET`]:
/// `<run_root>/<vm_id>/<fc_basename>/<vm_id>/root/firecracker.sock`.
///
/// `vm_id` appears twice because m80-jailer inherits Firecracker's jailer
/// convention of nesting `<chroot-base>/<exec-basename>/<id>/root/`.
/// `firecracker.sock` (16 bytes) is longer than `vsock.sock` (10 bytes),
/// so checking the API-socket path against `SUN_PATH_BUDGET` is conservative
/// for both sockets in the same jail.
pub(crate) fn socket_path_len(run_root: &Path, vm_id: &str, fc_basename: &str) -> usize {
    run_root.as_os_str().len()
        + 1 + vm_id.len()              // /<vm_id>
        + 1 + fc_basename.len()        // /<fc_basename>
        + 1 + vm_id.len()              // /<vm_id>
        + "/root/".len()               // /root/
        + FIRECRACKER_API_SOCKET.len() // firecracker.sock
}

/// Compute the Firecracker REST API socket path visible to the host.
///
/// The socket is created inside the jailer root, not directly in the run
/// directory.
#[must_use]
pub fn firecracker_api_socket_path(run_dir: &Path, firecracker_bin: &Path) -> PathBuf {
    m80_jailer::jail_root_path(run_dir, firecracker_bin).join(FIRECRACKER_API_SOCKET)
}

/// Compute the host-side vsock muxer socket path.
///
/// The socket is created inside the jailer root, not directly in the run
/// directory.
#[must_use]
pub fn vsock_socket_path(run_dir: &Path, firecracker_bin: &Path) -> PathBuf {
    m80_jailer::jail_root_path(run_dir, firecracker_bin).join(VSOCK_SOCKET)
}

/// Compute the per-VM writable root filesystem overlay path.
#[must_use]
pub fn rootfs_overlay_path(run_dir: &Path) -> PathBuf {
    run_dir.join(ROOTFS_OVERLAY_IMAGE)
}

/// Compute the per-VM writable workspace scratch image path.
#[must_use]
pub fn scratch_image_path(run_dir: &Path) -> PathBuf {
    run_dir.join(SCRATCH_IMAGE)
}

pub(crate) fn preallocated_drive_slot_filename(slot: u8) -> String {
    format!("{PREALLOCATED_DRIVE_SLOT_PREFIX}{slot}.raw")
}

pub(crate) fn preallocated_drive_slot_path(run_dir: &Path, slot: u8) -> PathBuf {
    run_dir.join(preallocated_drive_slot_filename(slot))
}

pub(crate) fn preallocated_drive_slot_jail_path(slot: u8) -> PathBuf {
    PathBuf::from(format!("/{}", preallocated_drive_slot_filename(slot)))
}

pub(crate) fn pmem_layer_backing_dir(run_dir: &Path) -> PathBuf {
    run_dir.join(PMEM_BACKING_DIR)
}

pub(crate) fn pmem_layer_backing_path(run_dir: &Path, slot: usize) -> PathBuf {
    pmem_layer_backing_dir(run_dir).join(format!("{slot}.img"))
}

pub(crate) fn pmem_layer_jail_basename(slot: usize) -> String {
    format!("{PMEM_JAIL_PREFIX}{slot}.img")
}

pub(crate) fn pmem_layer_jail_bind_dest(slot: usize) -> PathBuf {
    PathBuf::from(pmem_layer_jail_basename(slot))
}

pub(crate) fn pmem_layer_jail_path(slot: usize) -> PathBuf {
    PathBuf::from(format!("/{}", pmem_layer_jail_basename(slot)))
}

/// Compute the console log path for the VM.
#[must_use]
pub fn console_log_path(run_dir: &Path) -> PathBuf {
    run_dir.join(CONSOLE_LOG)
}

/// Compute the host-side path for Firecracker's native structured log.
#[must_use]
pub fn fc_log_path(run_dir: &Path, firecracker_bin: &Path) -> PathBuf {
    m80_jailer::jail_root_path(run_dir, firecracker_bin).join(FIRECRACKER_LOG)
}

/// Compute the jail-visible path for Firecracker's native structured log.
#[must_use]
pub(crate) fn fc_log_jail_path() -> PathBuf {
    PathBuf::from(format!("/{FIRECRACKER_LOG}"))
}

/// Compute the host-side path for Firecracker's native JSON metrics.
#[must_use]
pub fn fc_metrics_path(run_dir: &Path, firecracker_bin: &Path) -> PathBuf {
    fc_metrics_path_from_jail_root(&m80_jailer::jail_root_path(run_dir, firecracker_bin))
}

pub(crate) fn fc_metrics_path_from_jail_root(jail_root: &Path) -> PathBuf {
    jail_root.join(FIRECRACKER_METRICS)
}

/// Compute the jail-visible path for Firecracker's native JSON metrics.
#[must_use]
pub(crate) fn fc_metrics_jail_path() -> PathBuf {
    PathBuf::from(format!("/{FIRECRACKER_METRICS}"))
}

/// Compute the boot identity record path for the VM.
#[must_use]
pub fn boot_identity_path(run_dir: &Path) -> PathBuf {
    run_dir.join(BOOT_IDENTITY_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_len_matches_constructed_path() {
        let run_root = Path::new("/var/lib/m80-run");
        let vm_id = "persist-fs";
        let fc_basename = "firecracker";

        let constructed = run_root
            .join(vm_id)
            .join(fc_basename)
            .join(vm_id)
            .join("root")
            .join(FIRECRACKER_API_SOCKET);

        assert_eq!(
            socket_path_len(run_root, vm_id, fc_basename),
            constructed.as_os_str().len()
        );
    }

    #[test]
    fn socket_path_len_doubles_vm_id() {
        // Increasing vm_id by N bytes must increase path length by 2N
        // because the jail layout uses vm_id twice.
        let run_root = Path::new("/run/m80");
        let fc_basename = "firecracker";

        let short = socket_path_len(run_root, "abc", fc_basename);
        let longer = socket_path_len(run_root, "abcdefgh", fc_basename);
        assert_eq!(longer - short, 2 * (8 - 3));
    }

    #[test]
    fn socket_path_len_at_kernel_cap_with_default_run_root() {
        // /var/lib/m80-run (16) + firecracker basename (11): each extra vm_id
        // byte costs 2 path bytes (vm_id appears twice in the jail layout),
        // so the boundary is V=27 -> 106 (fits) and V=28 -> 108 (overflows).
        // No vm_id length produces exactly 107 with this run_root + basename.
        let run_root = Path::new("/var/lib/m80-run");
        let fc = "firecracker";
        let v27 = "a".repeat(27);
        let v28 = "a".repeat(28);
        assert_eq!(socket_path_len(run_root, &v27, fc), 106);
        assert!(socket_path_len(run_root, &v27, fc) <= SUN_PATH_BUDGET);
        assert_eq!(socket_path_len(run_root, &v28, fc), 108);
        assert!(socket_path_len(run_root, &v28, fc) > SUN_PATH_BUDGET);
    }

    #[test]
    fn socket_path_len_long_fc_basename_eats_budget() {
        // A long firecracker basename consumes budget that would otherwise
        // be available for the vm_id. Layout: /var/lib/m80-run/abc/<fake>/abc/root/firecracker.sock
        // = 16 + 1+3 + 1+33 + 1+3 + 1+4 + 1+16 = 80.
        let run_root = Path::new("/var/lib/m80-run");
        let fake = "fake-firecracker-cgroup-fail-1234";
        let vm_id = "abc";
        let len = socket_path_len(run_root, vm_id, fake);
        assert_eq!(len, 80);
        assert!(len < SUN_PATH_BUDGET);
    }

    #[test]
    fn pmem_layer_paths_are_slot_indexed_and_not_caller_derived() {
        let run_dir = Path::new("/run/m80/vm-a");

        assert_eq!(
            pmem_layer_backing_path(run_dir, 2),
            PathBuf::from("/run/m80/vm-a/pmem/2.img")
        );
        assert_eq!(pmem_layer_jail_bind_dest(2), PathBuf::from("pmem.2.img"));
        assert_eq!(pmem_layer_jail_path(2), PathBuf::from("/pmem.2.img"));
    }
}
