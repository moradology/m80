# firecracker-containerd: Rootfs Strategy Exploration

**Source**: https://github.com/firecracker-microvm/firecracker-containerd  
**Date**: 2026-05-04  
**Purpose**: Evaluate their per-VM rootfs construction patterns for applicability to m80's pivot from full file-copy to shared-RO-base + sparse overlay.

---

## 1. Snapshotter Strategy

firecracker-containerd is a containerd plugin and therefore inherits containerd's snapshotter abstraction. They document two host-side snapshotter implementations and one remote (in-VM) model:

### Naive snapshotter (`snapshotter/`)
A proof-of-concept that does a full copy per snapshot — no content deduplication. Validates integration correctness but is explicitly not production-intended. Analogous to m80's current 727 ms file-copy path.

### Devmapper snapshotter (`firecracker-control/cmd/containerd/main.go`, imports `containerd/containerd/snapshots/devmapper/plugin`)
The production-recommended approach. Uses Linux device-mapper thin provisioning:
- A **thin pool** (`fc-dev-thinpool`) is created from data/metadata backing files (100 GB data, 2 GB metadata in the quickstart; production uses real block devices).
- Each image layer is a **thin volume** — a CoW lid over the read-only base snapshot in the pool. The devmapper pool tracks dirty pages; unmodified pages alias the parent.
- `Prepare(key, parent)` creates a new thin device cloned from `parent`. `Commit(key)` freezes it read-only. `View(key, parent)` creates a read-only snapshot for inspection.
- The result is a `/dev/mapper/<name>` block device path. The runtime hands that path to Firecracker as a virtio-blk drive.
- Configuration (`docs/getting-started.md`):
  ```toml
  [plugins."io.containerd.snapshotter.v1.devmapper"]
    pool_name      = "fc-dev-thinpool"
    base_image_size = "10GB"
    root_path      = "/var/lib/firecracker-containerd/snapshotter/devmapper"
  ```
- The quickstart explicitly warns: "The configuration with loopback devices is slow and not intended for use in production." Production requires a real LVM thin pool on a block device.
- Concerns documented in `docs/snapshotter.md`: "file read/write/copy-on-write performance, as well as around provisioning and deactivation performance" — thin-device activation and teardown latency can dominate at high VM creation rates.

### Remote (in-VM) snapshotter (`snapshotter/`, `docs/remote-snapshotter.md`)
A more exotic model: the snapshotter runs **inside the microVM** rather than on the host. The host-side demux-snapshotter (`snapshotter/demux/snapshotter.go`) is a gRPC proxy that:
1. Extracts the containerd namespace from the request context (`getSnapshotterFromCache()`).
2. Forwards the snapshot call over vsock to the in-VM snapshotter.
3. Returns mount descriptors with VM-local identifiers (`mountutil.Map(mounts, vm.AddLocalMountIdentifier)`).

This model supports lazy-pulling snapshotters (stargz, nydus, overlaybd) running inside the VM. The constraint is "1 containerd namespace per microVM" because routing is namespace-keyed. The VM must boot before any snapshot is prepared — the sequence is inverted versus devmapper.

**Recommendation from the project**: devmapper for simple block-device use; remote snapshotter for lazy-pulling / eStargz workflows.

---

## 2. Host-Side Rootfs Construction

The flow for a standard devmapper deployment:

1. **Image pull** → containerd unpacks layers into devmapper thin volumes stacked on each other.
2. **Container create** → runtime calls `snapshotter.Prepare(containerKey, topLayerKey)` → devmapper creates a new thin device cloned from the top layer.
3. **Drive attachment** → runtime gets back `/dev/mapper/<containerKey>` as the mounts result, passes it to Firecracker's `PUT /drives/{id}` (or `PatchGuestDriveByID`) as a virtio-blk block device.
4. **VM boot** → Firecracker presents the block device as `vdb` (or whichever drive slot). The kernel boots off a separate rootfs drive (`vda`), then the container filesystem appears as a second drive inside the guest.

There is **no per-VM file copy**. The cost is one thin-device activation from the pool (sub-millisecond if the pool is on a real block device). All layers share pool pages; only written pages are private to that thin device.

For the base VM rootfs (not the container layer — the OS that runs the agent and runc), they use a **single shared squashfs image** (`docs/root-filesystem.md`, `tools/image-builder/Makefile`):

> "The filesystem must have the ability to run successfully from a read-only device, in order to prevent a VM from manipulating the filesystem in use by another VM."

The squashfs base is mounted read-only by Firecracker (`vda`). A per-VM writable overlay is provided by `overlay-init`.

---

## 3. Devmapper Mechanics in Detail

Thin provisioning via device-mapper is the closest existing open-source analogue to what m80 wants:

- **Thin pool** = the shared resource. Consists of a data device (holds actual blocks) and a metadata device (holds the block allocation B-tree). Created once per host with `dmsetup`.
- **Thin volume** = a virtual block device. Initially zero-sized; blocks are allocated on first write. A clone of another thin volume starts sharing all its blocks — clone creation is O(1) metadata.
- **Snapshot chain**: base image → layer 1 → layer 2 → per-VM writable volume. Each link is an O(1) clone. The pool handles CoW at 128-sector granularity.
- **Exposed surface**: each volume is a `/dev/mapper/<name>` block device. No mount required on the host; the device is passed directly to Firecracker.
- **Teardown**: `dmsetup remove <name>` deactivates the thin device. Blocks return to the pool lazily (background discard).

The concern the project documents — "provisioning and deactivation performance" — comes from thin-device activation taking non-trivial time when the metadata device is under contention. At >100 VMs/s creation rate this becomes a bottleneck.

---

## 4. In-Guest Setup

### VM-level rootfs (the OS layer)
The image builder (`tools/image-builder/`) produces a **squashfs** image containing the agent, runc, systemd, and supporting libraries. Build: `mksquashfs "$(WORKDIR)" rootfs.img -noappend`. The squashfs is mounted **read-only** by Firecracker as the root drive.

A custom init binary `overlay-init` (`tools/image-builder/files_debootstrap/sbin/overlay-init`) runs as PID 1 via the kernel parameter `init=/sbin/overlay-init`. It:
1. Mounts the squashfs-backed root as a read-only lower layer.
2. Creates a **tmpfs** as the writable upper layer (default), OR mounts a provided ext4 block device (`overlay_root=vdc` kernel param) for persistence.
3. Sets up an overlayfs with lower=squashfs, upper=tmpfs/ext4, work=workdir.
4. Calls `pivot_root` to switch into the overlay.
5. `exec /usr/sbin/init` (systemd) to continue boot.

This means the VM OS itself is CoW-on-tmpfs, not a copy. Multiple VMs share one squashfs blob on the host; each gets its own private tmpfs dirty layer.

### Container-level rootfs (the workload layer)
Inside the VM, the agent runs `runc` against the container bundle. The container rootfs arrives as a virtio-blk device (`vdb`, `vdc`, ...) provisioned by the host-side devmapper snapshot. The agent's drive-mount service (`agent/main.go` registers `drivemount.RegisterDriveMounterService`) handles attaching these drives inside the guest.

No initrd or switch_root magic for containers — they're just block devices attached to the already-running VM.

---

## 5. Latency Claims

firecracker-containerd's own documentation contains **no latency benchmarks**. The upstream Firecracker project claims:

- **< 125 ms** cold boot on i3.metal with default microVM size (128 MiB RAM, 1 vCPU).
- **< 5 MiB** memory overhead per microVM.
- **Up to 150 microVMs/second** creation rate on a single host.

The NSDI 2020 paper ("Firecracker: Lightweight Virtualization for Serverless Applications") contains concrete Lambda numbers but is not publicly crawlable. Known data points from secondary sources:
- AWS Lambda cold starts are reported as "under 100 ms to over 1 second" at the user-observable level, with Firecracker boot itself being the minority of that budget.
- Firecracker's snapshot restore uses `MAP_PRIVATE` on the memory file — pages are demand-faulted rather than preloaded. This means restore latency can be sub-100 ms even for a 512 MiB VM (only the kernel+VMM state file is fully deserialized; RAM pages come in on first access).
- Diff snapshots (dirty-page only) exist for checkpoint-restore workflows; the `mincore(2)` and KVM dirty-log tracking mechanisms are documented in `firecracker/docs/snapshotting/snapshot-support.md`.

---

## 6. What Is Portable to m80

m80 is not containerd-shaped, but three patterns are directly extractable:

### Pattern A: squashfs + overlayfs init (lift directly)
**Ref**: `tools/image-builder/files_debootstrap/sbin/overlay-init`, `docs/root-filesystem.md`

m80 already builds an ext4 base image. The firecracker-containerd model says: convert that base to squashfs (or keep it ext4 read-only), mount it read-only from Firecracker, and run `overlay-init` as PID 1 to pivot into an overlayfs. The writable upper layer is a tmpfs by default — zero allocation cost, zero copy, bounded by guest RAM. For workspaces that need persistence across runs, pass `overlay_root=<driveN>` and back that with a sparse ext4 provisioned on the host.

This eliminates the 727 ms file-copy entirely. The squashfs base is shared; the tmpfs upper is ephemeral; the workspace drive (if any) is a pre-provisioned sparse file mounted as a second virtio-blk device.

m80 already has `m80-storage` and `m80-image-manifest`. The change is:
- Ship the base image as squashfs (or ext4 with `ro` flag to Firecracker).
- Add `overlay-init` as PID 1 in the guest (a ~100-line shell script).
- Remove the `cp` in the launch path.

### Pattern B: devmapper thin-pool for writable-overlay persistence
**Ref**: `docs/getting-started.md`, `docs/snapshotter.md`

If m80 needs persistent per-VM workspace overlays (rather than tmpfs) at high creation rates, the devmapper thin-pool model is the right host-side mechanism. Each VM gets a thin device cloned from a base snapshot — O(1) creation, CoW at 128-sector granularity, no file copy. The `/dev/mapper/<name>` path is handed to Firecracker as a drive.

The complexity cost is real: dmsetup/LVM setup, pool management, device teardown. Only worth it if tmpfs upper layers are too small (workspace data exceeds available RAM) or if overlay persistence across VM restarts is required.

### Pattern C: Firecracker snapshot + MAP_PRIVATE restore for warm-pool
**Ref**: `firecracker/docs/snapshotting/snapshot-support.md`

For m80's warm-pool use case: snapshot a fully booted idle VM, store the memory file. Restore with `MAP_PRIVATE` — pages fault in on demand. This gives sub-100 ms apparent boot for the second and subsequent invocations from the same base. The memory file can be shared across N restore instances if each gets its own `MAP_PRIVATE` mapping (the OS won't CoW unless the guest writes those pages). This is essentially what AWS Lambda does for warm-start acceleration.

---

## Bonus: AWS Lambda Firecracker Tricks

From public documentation and the Firecracker paper:

- **Snapshot-and-restore**: Lambda snapshots a "base" microVM after language runtime initialization. Subsequent invocations restore from that snapshot rather than cold-booting. The restore uses `MAP_PRIVATE` on the memory blob — dirty pages become private; clean pages are shared across concurrent restores of the same base.
- **Memory deduplication via KSM**: The kernel's KSM (kernel same-page merging) can deduplicate identical pages across hundreds of VMs sharing the same base image. This is separate from CoW — KSM operates after the fact on already-private pages that happen to be identical.
- **Diff snapshots**: For Lambda's "warm" invocation model, only the dirty pages since the last invocation are written to a new diff snapshot. This keeps snapshot size proportional to what the function actually touched, not its total RAM.
- **"Snapchange"-style cloning**: Described in research as forking the memory state of a running VM into N copies simultaneously — each copy starts diverging from the same point. This requires userspace-managed memory sharing (UFFD + `MAP_SHARED` + CoW). Firecracker supports the primitives; the orchestration layer above it assembles the pattern.

The key insight: firecracker-containerd solves the **storage** problem (rootfs layers, CoW block devices). AWS Lambda solves the **memory** problem (snapshot, restore, dedup). m80 needs the storage solution first; the memory solution is a later optimization once the VM lifecycle is otherwise clean.

---

## Summary Table

| Problem | Their solution | m80 analogue | Lift? |
|---|---|---|---|
| Shared base rootfs | squashfs + read-only mount | ext4 with `ro` flag or squashfs | Yes — immediate |
| Per-VM writable layer | tmpfs upper via overlay-init | same | Yes — add overlay-init |
| Persistent workspace | devmapper thin volume | sparse ext4 + second drive | Yes — sparse pre-alloc |
| Container layer delivery | devmapper snapshot → `/dev/mapper/` | n/a (m80 has no container layers) | No |
| Lazy content pull | remote snapshotter + vsock | n/a | No |
| Warm start | Firecracker snapshot + MAP_PRIVATE | future m80 warm pool | Later |
