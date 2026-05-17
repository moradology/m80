# Pmem Layers

Beads: `m80-q420k.2.10`, `m80-q420k.3.3`, `m80-q420k.3.5`

`SandboxConfig::pmem_layers` declares read-only erofs images delivered to the
guest through Firecracker virtio-pmem and mounted before the caller receives a
`RunningSandbox`. `PmemSharing::PerVm` keeps per-VM host backing identity;
`PmemSharing::Shared(TrustDomainAck)` deliberately reuses one canonical
content-addressed backing inode for same-trust-domain guests.

## pmem-layer-host-pipeline

The cold-launch path is slot-indexed from admission through guest mount:

1. `Sandbox::launch` validates `SandboxConfig::pmem_layers` before creating a
   run directory. Invalid digests, mount paths, duplicates, or too many layers
   fail as typed `FcError::Config`/`ConfigError` values before any VMM side
   effect.
2. `phase_3_storage_prep` calls `phase_3b_resolve_pmem_backings`, resolves
   each `ErofsImageRef` digest through `m80-image-store`, and chooses the host
   backing from `PmemSharing`: `PerVm` copies or reflinks the store image to
   `<run_dir>/pmem/<slot>.img`, while `Shared` uses the canonical store artifact
   path directly and creates an active-use marker under
   `<store>/shared/<digest>/refs/<vm_id>`.
3. Jailer materialization binds each backing read-only at jail root destination
   `pmem.<slot>.img`, visible to Firecracker inside the jail as
   `/pmem.<slot>.img`.
4. `phase_11_rest_puts` emits one read-only Firecracker `PUT /pmem/pmem_<slot>`
   after preallocated hotplug drive slots and before the optional network
   interface, entropy device, and vsock.
5. After `phase_12b_ready_accept`, `phase_13_pmem_guest_mount` sends
   `PmemMountRequest` to guestd. Each spec maps slot `<N>` to `/dev/pmem<N>`,
   the admitted `GuestMountPath`, and the opaque erofs image digest.

The caller cannot choose the Firecracker pmem id, jail-visible path, host
backing path, device size, or any kernel command-line token.

`m80-image-store` is the erofs compatibility boundary. `import_existing` runs
`dump.erofs -s` for `ImageKind::Erofs` and rejects artifacts that declare
features outside the guest-pinned floor: `sb_csum`, `mtime`, `0padding`, and
LZ4/LZ4HC compression. Unsupported feature tokens such as `chunked_file`, or
unsupported compressors such as zstd, lzma, or deflate, fail before the digest
is stored and before any VM can attach the artifact.

## per-vm-isolation

`PmemSharing::PerVm` always materializes a per-VM host backing under that VM's
run directory:

```text
<run_root>/<vm_id>/pmem/<slot>.img
```

Two VMs declaring the same image digest get different run directories and
therefore different backing inodes. This is the safety property that keeps
PerVm from requiring a same-trust-domain witness. Reflink-capable hosts may
share extents below the filesystem layer, but the Firecracker pmem backing file
identity is per VM.

Pinned tests:

- `crates/m80-firecracker/src/storage_prep.rs::tests::pmem_backing_prep_clones_per_vm_files_with_distinct_inodes`
- `crates/m80-firecracker/tests/pmem_layer_real_kvm.rs::pmem_per_vm_backings_have_distinct_inodes` (Phase B e2e final name)

## shared-trust-domain-reuse

`PmemSharing::Shared(TrustDomainAck)` is an explicit same-trust-domain
optimization. Storage prep resolves the requested digest through
`m80-image-store` and binds the canonical erofs artifact path directly:

```text
<image-store>/<digest[0..2]>/<digest>/image.erofs
```

Two VMs declaring `Shared` for the same digest therefore observe the same host
backing inode. `PerVm` never takes this path, even when the digest matches a
different VM's declaration. If the shared artifact is missing, launch fails with
the typed image-store `NotFound` path; it does not silently clone or fall back
to `PerVm` semantics.

The shared artifact is a canonical image-store input, not a temporary backing.
m80 therefore does not delete it when the last VM stops. Active-use markers
track which VM ids currently hold shared refs. `RunningSandbox::stop` and
`force_kill` release those markers after the jail has been dropped and its bind
mounts are gone. `Backend::new` sweeps stale markers for VM ids no longer
present under the run root.

Pinned tests:

- `crates/m80-image-store/tests/store.rs::shared_ref_acquire_and_release_updates_marker_count_without_deleting_artifact`
- `crates/m80-image-store/tests/store.rs::sweep_shared_refs_removes_only_non_live_markers`
- `crates/m80-firecracker/src/storage_prep.rs::tests::shared_pmem_backing_reuses_store_inode_without_clone`
- `crates/m80-firecracker/src/storage_prep.rs::tests::shared_pmem_backing_is_idempotent_under_concurrent_attach`
- `crates/m80-firecracker/src/storage_prep.rs::tests::mixed_pmem_sharing_keeps_per_vm_clone_and_shared_store_inode`
- `crates/m80-firecracker/src/storage_prep.rs::tests::missing_shared_pmem_image_fails_before_backing_dir_created`
- `crates/m80-firecracker/tests/pmem_layer_real_kvm.rs::pmem_layer_shared_reuses_backing_inode_real_kvm`

## dax-flag

Guestd mounts each requested pmem device as erofs with DAX and then reads
`/proc/mounts`. It succeeds only when the mounted target line matches the
requested `/dev/pmem<N>` source, filesystem `erofs`, and an option token of
`dax` or `dax=<value>`. Missing DAX returns `PmemMountError::DaxFlagAbsent`,
which host launch maps to `FcError::PmemMount`.

The real-KVM smoke for `m80-q420k.2.9` observed:

```text
/dev/pmem0 /opt/m80-layers/smoke erofs ro,nosuid,nodev,relatime,user_xattr,acl,cache_strategy=readaround,dax=always 0 0
```

Pinned tests:

- `crates/m80-guestd/src/connection/pmem/tests.rs::dax_absent_after_mount_fails_closed`
- `crates/m80-firecracker/src/lifecycle/pmem.rs::tests::failed_dax_response_maps_to_typed_error`
- `crates/m80-firecracker/tests/pmem_layer_real_kvm.rs::pmem_layer_mounts_erofs_dax_before_workload` (Phase B e2e final name)

## ro-only

Pmem layers are read-only at both layers of the boundary:

- Firecracker receives `PmemConfig { read_only: true, root_device: false }`.
- Guestd calls `mount(2)` with filesystem `erofs`, flags
  `MS_RDONLY | MS_NOSUID | MS_NODEV`, and option `dax=always`.

Pinned tests:

- `crates/m80-firecracker/src/preboot_tests.rs::pmem_puts_follow_hotplug_slots_and_precede_network_interface`
- `crates/m80-guestd/src/connection/pmem/tests.rs::mounts_erofs_with_dax_and_reports_mounted`

## fail-closed-points

The pmem layer path has explicit fail-closed boundaries:

- bad digest: `ImageDigest::parse` rejects non-lowercase-hex or wrong length
- missing image: image-store resolution fails as `FcError::ImageStore`
- escape mount path: `GuestMountPath::parse` rejects non-`/opt/m80-layers/<name>` paths
- shadowing mount path: reserved guest roots such as `/proc`, `/sys`, `/dev`,
  `/etc`, `/workspace`, `/lower`, `/upper`, `/merged`, and `/snapshot` are
  rejected before storage prep
- duplicated mount path: `validate_pmem_layers` rejects duplicates before
  run-dir creation
- oversize image: storage prep rejects a resolved erofs artifact that cannot fit
  in Firecracker v1.15.1's 512 GiB virtio-pmem guest-physical window before
  per-VM clone creation, Shared active-use marker creation, jail binding, or
  Firecracker REST admission
- backing mismatch: preboot planning rejects any layer/backing count mismatch
  before REST PUTs
- guest mount failure: guestd returns `PmemMountError::MountFailed`
- DAX absent after mount: guestd returns `PmemMountError::DaxFlagAbsent`

Pinned tests:

- `crates/m80-firecracker/tests/pmem_layers_fail_closed.rs` (Phase B scenario 5 final file)
- `crates/m80-firecracker/src/launch/tests.rs::launch_rejects_duplicate_pmem_mounts_before_run_dir_creation`
- `crates/m80-firecracker/src/storage_prep.rs::tests::pmem_device_size_validation_accepts_firecracker_window_edge`
- `crates/m80-firecracker/src/storage_prep.rs::tests::pmem_device_size_validation_rejects_over_firecracker_window`
- `crates/m80-firecracker/src/preboot_tests.rs::pmem_plan_requires_resolved_backings`
- `crates/m80-guestd/src/connection/pmem/tests.rs::invalid_device_path_fails_before_waiting`
- `crates/m80-guestd/src/connection/pmem/tests.rs::invalid_mount_path_fails_before_mounting`
- `crates/m80-guestd/src/connection/pmem/tests.rs::missing_device_waits_for_pmem_devname_and_fails_closed`

## no-leak-teardown

Pmem backing files live below the owned per-VM run directory. A normal
`StoppedSandbox::delete()` removes the whole run directory, including:

```text
<run_root>/<vm_id>/pmem/<slot>.img
```

Launch failure preserves or deletes the same run directory through the existing
launch cleanup guard policy, so pmem backings are not a separate cleanup
surface. The Phase B real-KVM no-leak test is
`crates/m80-firecracker/tests/pmem_layer_real_kvm.rs::pmem_teardown_removes_backing_files`.

Related behavior captures:

- `docs/behaviors/jailer/pmem-bindings.md`
- `docs/behaviors/lifecycle/pmem-preboot.md`
- `docs/behaviors/guestd/pmem-dax-mount.md`
- `docs/behaviors/image-build/pmem-erofs-dax-kernel.md`
