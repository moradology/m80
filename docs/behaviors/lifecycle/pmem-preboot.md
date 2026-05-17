# Pmem Preboot PUTs

Bead: `m80-q420k.2.8`

Declared `SandboxConfig::pmem_layers` become Firecracker pmem devices only
through the phase-11 preboot plan. Storage prep must first resolve one
`ResolvedPmemBacking` per declared layer; if the counts diverge, preboot
planning fails with `FcError::InvalidState` instead of issuing a partial device
plan.

The PUT order is fixed:

1. machine config
2. boot source
3. rootfs drive
4. rootfs overlay drive
5. optional workspace drive
6. preallocated hotplug drive slots
7. pmem layers in declared order
8. optional network interface
9. entropy device
10. vsock

Each pmem PUT uses only slot-derived identity:

- Firecracker resource id: `pmem_<slot>`
- jail-visible backing path: `/pmem.<slot>.img`
- `root_device = false`
- `read_only = true`

The caller's `PmemLayer` can choose the validated erofs image digest and guest
mount destination, but it cannot choose the Firecracker id, host path, device
size, or kernel command-line tokens. Firecracker derives the pmem device size
from the bound backing file. For the pinned Firecracker v1.15.1 substrate, m80
rejects resolved erofs artifacts whose backing length cannot fit in the 512 GiB
virtio-pmem guest-physical window before clone, Shared marker, jail bind, or
REST admission.

After guestd readiness, cold launch runs `phase_13_pmem_guest_mount`. That
phase asks guestd to mount each slot-indexed `/dev/pmem<N>` as read-only erofs
with `dax=always` at the corresponding `GuestMountPath`. Pmem is still rejected
on snapshot restore until restore-specific remount semantics land.

Tests:

- `crates/m80-firecracker/src/preboot_tests.rs::pmem_puts_follow_hotplug_slots_and_precede_network_interface`
- `crates/m80-firecracker/src/preboot_tests.rs::pmem_puts_are_omitted_when_layers_are_empty`
- `crates/m80-firecracker/src/preboot_tests.rs::pmem_plan_requires_resolved_backings`
- `crates/m80-firecracker/src/preboot_tests.rs::pmem_plan_does_not_add_kernel_cmdline_pmem_tokens_or_size`
- `crates/m80-firecracker/src/lifecycle/pmem.rs::tests::specs_use_slot_indexed_devices_and_layer_contract`
- `crates/m80-firecracker/src/lifecycle/pmem.rs::tests::failed_dax_response_maps_to_typed_error`

Smoke:

- This path touches Firecracker preboot device configuration. Closing the bead
  requires a real-KVM run with at least one declared `PmemLayer`, so the new
  `PUT /pmem/{id}` request reaches Firecracker and the guest reports the erofs
  DAX mount before the workload runs.
