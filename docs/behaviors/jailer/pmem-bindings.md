# Pmem Backing Binds

Beads: `m80-q420k.2.7`, `m80-q420k.3.4`

Declared pmem layers are never allowed to choose their Firecracker-visible jail
paths. `SandboxConfig::pmem_layers` carries an image digest and a guest mount
path; `m80-firecracker` derives the host and jail paths from the layer slot.

For each `PmemSharing::PerVm` layer, storage prep resolves the erofs digest via
`m80-image-store`, reflink-clones the stored artifact to
`<run_dir>/pmem/<slot>.img`, and records a `ResolvedPmemBacking` with:

- `host_path = <run_dir>/pmem/<slot>.img`
- `jail_basename = pmem.<slot>.img`
- `sharing = PmemSharing::PerVm`

For each `PmemSharing::Shared(_)` layer, storage prep resolves the erofs digest
to the canonical image-store artifact and records a `ResolvedPmemBacking` with:

- `host_path = /var/lib/m80-images/<prefix>/<digest>/image.erofs`
- `jail_basename = pmem.<slot>.img`
- `sharing = PmemSharing::Shared(_)`

Jailer materialization binds each backing read-only at the relative jail
destination `pmem.<slot>.img`, which is visible to Firecracker inside the
chroot as `/pmem.<slot>.img`. The destination is generated only from the slot
index; caller-controlled strings do not flow into the jail path.

Shared backings use `BindMode::RoImageStore`, not plain `Ro`. `Plan::compute`
rejects a `RoImageStore` source outside `/var/lib/m80-images`, and
materialization revalidates the canonicalized source against the canonical
image-store root before calling `mount(2)`. `RoImageStore` remounts with the
same read-only bitset as normal read-only binds:
`MS_BIND|MS_REMOUNT|MS_NODEV|MS_NOEXEC|MS_NOSUID|MS_RDONLY`.

`m80-jailer::Plan::compute` rejects absolute, empty, parent-traversing, hidden
kernel filesystem, and duplicate bind destinations before any mount call. A
`CreateInsideJail` entry may share a destination with a later bind because that
is the explicit snapshot mount-point pattern; two real bind mounts may not
share a destination.

Tests:

- `crates/m80-firecracker/src/storage_prep.rs::tests::pmem_backing_prep_clones_per_vm_files_with_distinct_inodes`
- `crates/m80-firecracker/src/launch/tests.rs::pmem_backings_are_bound_ro_to_slot_jail_paths`
- `crates/m80-firecracker/src/layout.rs::tests::pmem_layer_paths_are_slot_indexed_and_not_caller_derived`
- `crates/m80-jailer/tests/plan_compute.rs::duplicate_bind_dest_is_rejected`
- `crates/m80-jailer/tests/plan_compute.rs::pmem_dest_collisions_with_standard_assets_are_rejected`
- `crates/m80-jailer/tests/plan_compute.rs::pmem_bind_dest_at_jail_root_is_accepted`
- `crates/m80-jailer/tests/plan_compute.rs::pmem_bind_dest_cannot_escape_with_parent_component`
- `crates/m80-jailer/src/plan.rs::tests::shared_image_store_bind_remount_flags_include_readonly`
- `crates/m80-jailer/tests/plan_compute.rs::shared_image_store_bind_source_outside_default_store_is_rejected`
- `crates/m80-jailer/tests/plan_compute.rs::shared_image_store_bind_keeps_same_source_while_per_vm_paths_differ`
- `crates/m80-firecracker/src/launch/tests.rs::shared_pmem_backings_are_bound_with_image_store_ro_mode`
- `crates/m80-firecracker/tests/pmem_layer_real_kvm.rs::pmem_layer_shared_reuses_backing_inode_real_kvm`

Smoke:

This behavior changes the bind-mount plan, so closing the bead requires a
green real-KVM run on a host with `/dev/kvm`. For the shared-pmem path, the
real-KVM assertion is guest-side `/proc/mounts`: `assert_pmem_mounts` requires
the shared mount line to include `ro` and `dax`/`dax=...`, while host-side
metadata proves both jailed paths resolve to the same image-store inode.
