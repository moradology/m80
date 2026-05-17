# Pmem DAX Mounts

Bead: `m80-q420k.2.9`

Cold launch attaches each declared pmem layer before `InstanceStart`, then
asks guestd to mount those devices after the ready signal and before returning
`RunningSandbox`. The host request is `PmemMountRequest`; each spec carries
only VM-mechanics data:

- guest block device path: `/dev/pmem<N>`
- guest mount path: the admitted `/opt/m80-layers/<name>` path
- erofs image digest: opaque to the guest, included for diagnostics/pairing

Guestd validates the device path as `/dev/pmem<N>` and the mount path as an
accepted pmem layer destination. It waits briefly for the block device when it
is not already present, creates the mount directory, and calls `mount(2)` with
filesystem `erofs`, flags `MS_RDONLY | MS_NOSUID | MS_NODEV`, and option
`dax=always`.

After mount, guestd reads `/proc/mounts` and fails closed unless the target is
mounted from the requested device as `erofs` and the option list contains
`dax` or `dax=<value>`. A pre-existing matching erofs+DAX mount returns
`AlreadyMounted`; a pre-existing non-matching mount returns `MountFailed`.

Typed failures are reported per device as `PmemMountError`:

- `DeviceNotFound`
- `MountFailed`
- `DaxFlagAbsent`
- `InvalidDevicePath`
- `InvalidMountPath`
- `Io`

The host treats any failed status as `FcError::PmemMount` and fails launch
before exposing a running handle. Protocol mismatches, missing statuses, or
wrong request ids remain `FcError::Protocol`.

Tests:

- `crates/m80-guestd/src/connection/pmem/tests.rs::mounts_erofs_with_dax_and_reports_mounted`
- `crates/m80-guestd/src/connection/pmem/tests.rs::dax_absent_after_mount_fails_closed`
- `crates/m80-guestd/src/connection/pmem/tests.rs::already_mounted_with_dax_is_noop`
- `crates/m80-firecracker/tests/pmem_preboot_real_kvm.rs::pmem_layer_real_kvm_mounts_erofs_dax_before_workload`
