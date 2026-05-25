# VM Memory Scaling

Behavior capture for `m80-2ggw.5.6`.

## Decision

m80 does not expose runtime memory scaling in v0.x. VM memory is selected at
launch with `SandboxConfig::mem_size_mib` and stays fixed for the VM lifetime.

Firecracker supports memory hotplug, but m80 deliberately does not wire it into
the public surface. Hotplug changes the guest memory shape after boot, requires
guest kernel support such as `CONFIG_MEMORY_HOTPLUG`, and depends on guest-side
ACPI plumbing. m80's stripped kernel keeps ACPI out of the fast boot contract,
and current callers do not have a measured workload that needs per-request
runtime memory reshaping.

This decision is separate from m80's existing read-only erofs-over-pmem layer
surface. `SandboxConfig::pmem_layers` uses Firecracker `PUT /pmem` to attach
bounded, read-only image-store artifacts as virtio-pmem devices and then mounts
them as erofs+DAX layers inside the guest. That is a file/content distribution
mechanism, not runtime memory expansion.

## Non-Goals

- No memory hotplug API in `SandboxConfig`.
- No balloon memory reclaim. `AGENTS.md` documents virtio-balloon as out of
  scope because it depends on a trusted guest driver.
- No general writable pmem memory-expansion surface.
- No automatic switch from explicit `mem_size_mib` sizing to host-driven memory
  overcommit.

## Revisit Triggers

Revisit this only if a concrete caller brings measured evidence that fixed
`mem_size_mib` sizing is the bottleneck. Plausible triggers:

- warm-pool oversubscription needs per-request memory shaping,
- a production workload shows large idle-memory waste that cannot be solved by
  separate pool classes,
- Firecracker and the stripped guest kernel can support a hotplug path without
  regressing the boot-latency and guest-surface goals.

Until one of those is true, the supported answer is explicit memory classes via
`mem_size_mib`.

## Verification

- `AGENTS.md` records virtio-balloon as a non-goal.
- `docs/behaviors/rootfs/pmem-layers.md` records the existing read-only
  erofs-over-pmem layer contract.
- `docs/behaviors/kernel/config-completeness.md` records balloon kernel options
  as deliberately absent.
