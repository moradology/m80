# Kata Containers: Rootfs and Overlayfs Exploration

**Goal:** Understand how Kata Containers solves the shared-base + per-container writable overlay
problem so m80 can make an informed design decision. m80's situation: Firecracker, virtio-blk only
(no virtiofs), and a 727 ms full-copy baseline we want to eliminate.

Sources examined: live GitHub main branch as of 2026-05-04.

---

## 1. Image-to-Rootfs Path

Kata uses a two-layer concept that is easy to confuse:

- **Guest VM image** — a minimal Linux rootfs (Ubuntu/Alpine) that boots the VM and runs
  `kata-agent`. This is _not_ the container workload image. On QEMU it is mapped via DAX/pmem
  (`/dev/pmem0`) so the host file is directly paged into the guest without a copy; on other
  hypervisors it may be a plain disk image. This layer is shared across all VMs; each VM does _not_
  get its own copy.

- **Container rootfs** — the OCI image layers that become the container's `/`. This is what we
  actually care about. Kata supports three delivery modes, chosen by the configured snapshotter:

  | Snapshotter | Host-side representation | In-VM delivery |
  |---|---|---|
  | overlayfs (default) | Host overlayfs dirs | `virtiofsd` shares the merged tree via virtio-fs |
  | devicemapper | Dedicated block device per container (CoW thin provision) | Passed as virtio-blk/scsi; mounted directly in-guest as the rootfs, no in-guest overlayfs |
  | EROFS (containerd 2.1+) | Per-layer `.erofs` files | Each layer passed as read-only virtio-blk device; in-guest overlayfs assembles them |

  The key insight: Kata does **not** create a full per-container disk image copy. The writable layer
  either comes from devicemapper CoW on the host (a thin provision, not a copy) or from an
  ephemeral ext4 upper layer created fresh (zero bytes on disk, grows as writes arrive).

---

## 2. In-Guest Overlayfs: kata-agent Rust Source

All source references are to `main` branch, 2026-05-04.

### Two distinct call paths

**Path A — OCI/runc-compatible containers (per-process namespace)**

File: `src/agent/rustjail/src/mount.rs`
File: `src/agent/rustjail/src/container.rs`

`container.rs` orchestrates the child process setup (`do_init_child`, line 638). The rootfs comes
in pre-assembled (by the host snapshotter, typically via virtiofs) at a path specified in the OCI
spec. The sequence is:

```
// container.rs ~line 587-650
let rootfs = spec.root().as_ref().unwrap().path().display().to_string();
let root = fs::canonicalize(&rootfs)?;
let rootfs = root.to_str().unwrap();

// Inside CLONE_NEWNS:
mount::init_rootfs(cfd_log, &spec, &cgroup_paths, &cgroup_mounts, bind_device)?;

// Then, after hooks:
if no_pivot {
    mount::ms_move_root(rootfs)?;   // MS_MOVE variant, no pivot_root syscall
} else {
    mount::pivot_rootfs(rootfs)?;   // fd-based pivot_root
}
mount::finish_rootfs(cfd_log, &spec, &oci_process)?;
```

`init_rootfs` (`mount.rs` line 167) bind-mounts the rootfs onto itself, sets propagation, and
processes the OCI spec's `mounts` array (proc, sysfs, cgroup, bind mounts) into the rootfs
directory. It does NOT set up the overlayfs itself — that is done by the storage subsystem before
this point, or by the host snapshotter.

### pivot_root sequence (`mount.rs` line 523)

```rust
pub fn pivot_rootfs<P: ?Sized + NixPath + std::fmt::Debug>(path: &P) -> Result<()> {
    let oldroot = fcntl::open("/", OFlag::O_DIRECTORY | OFlag::O_RDONLY, Mode::empty())?;
    defer!(unistd::close(oldroot).unwrap());
    let newroot = fcntl::open(path, OFlag::O_DIRECTORY | OFlag::O_RDONLY, Mode::empty())?;
    defer!(unistd::close(newroot).unwrap());

    // fchdir into the new root so pivot_root acts on it
    unistd::fchdir(newroot)?;
    pivot_root(".", ".")
        .context(format!("failed to pivot_root on {path:?}"))?;

    // fchdir back to oldroot (kernel contract: cwd is now oldroot after pivot_root(".",".")
    unistd::fchdir(oldroot)?;

    // rslave so our unmounts don't propagate to the host (avoids devicemapper races)
    mount(Some("none"), ".", Some(""), MsFlags::MS_SLAVE | MsFlags::MS_REC, Some(""))?;

    // MNT_DETACH lets us unmount /proc/self/cwd
    umount2(".", MntFlags::MNT_DETACH)?;

    unistd::chdir("/")?;
    stat::umask(Mode::from_bits_truncate(0o022));
    Ok(())
}
```

The `pivot_root(".", ".")` trick (both arguments are the new root) is lifted directly from runc:
open both old and new root as FDs first, fchdir to the new root, call `pivot_root(".", ".")`, then
use the saved FD to fchdir back to old root, make it rslave, and detach-unmount it.

**Path B — EROFS multi-layer storage (the more interesting case for m80)**

File: `src/agent/src/storage/multi_layer_erofs.rs`
File: `src/agent/src/storage/fs_handler.rs`

This is the path that maps most closely to m80's problem.

### EROFS + OverlayFS in-guest assembly (`multi_layer_erofs.rs`)

The host runtime passes each image layer as a separate virtio-blk (or SCSI) block device tagged with
custom storage options (`X-kata.overlay-upper`, `X-kata.overlay-lower`, `X-kata.multi-layer=true`).
The agent's `add_storages` (storage/mod.rs) detects the multi-layer marker and calls
`handle_multi_layer_erofs_group`.

Core assembly sequence (`multi_layer_erofs.rs`, approximately lines 172–295):

```rust
// 1. Create temp mount tree under /run/kata-containers/<cid>/multi-layer/
let temp_base = PathBuf::from(format!("/run/kata-containers/{}/multi-layer", cid_str));
fs::create_dir_all(&temp_base)?;

let upper_mount = temp_base.join("upper");
fs::create_dir_all(&upper_mount)?;

// 2. Mount the ext4 rw block device at upper_mount/
//    (wait_and_mount_layer resolves SCSI or virtio-blk PCI address → /dev/vdX)
wait_and_mount_layer(ext4_storage, &upper_mount, sandbox, &logger).await?;

// 3. Mount each EROFS read-only block device at upper_mount/lower-N/
let mut lower_mounts = Vec::new();
for (index, erofs) in erofs_storages.iter().enumerate() {
    let lower_mount = temp_base.join(format!("lower-{}", index));
    fs::create_dir_all(&lower_mount)?;
    wait_and_mount_layer(erofs, &lower_mount, sandbox, &logger).await?;
    lower_mounts.push(lower_mount);
}

// 4. Extract upperdir and workdir from inside the mounted ext4 device
let upperdir = upper_mount.join("upper");   // directory within the ext4 volume
let workdir  = upper_mount.join("work");
if !upperdir.exists() { fs::create_dir_all(&upperdir)?; }
fs::create_dir_all(&workdir)?;

// 5. Build lowerdir= colon-separated list (outermost first)
let lowerdir = lower_mounts.iter()
    .map(|p| p.display().to_string())
    .collect::<Vec<_>>()
    .join(":");

// 6. Mount overlay
let overlay_options = format!(
    "upperdir={},lowerdir={},workdir={}",
    upperdir.display(), lowerdir, workdir.display()
);
baremount(
    Path::new("overlay"),
    Path::new(&ext4_storage.mount_point),  // the final container rootfs path
    "overlay",
    nix::mount::MsFlags::empty(),
    &overlay_options,
    &logger,
)?;
```

The `wait_and_mount_layer` function resolves the device path by waiting for uevents from the kernel
(hotplug path), then calls `baremount` with the layer's fstype (erofs / ext4).

After `add_storages` completes, the OCI container setup in `rustjail` runs `init_rootfs` and
`pivot_rootfs` as described in Path A above.

### Simple overlayfs (non-multi-layer) — `fs_handler.rs`

For the non-EROFS path where the host sends a plain overlayfs storage descriptor with option
`io.katacontainers.fs-opt.overlay-rw`, the agent creates `upper/` and `work/` directories under
`/run/kata-containers/<cid>/`, appends them to the storage options, and then calls
`common_storage_handler` which does a single `mount(2)` with the assembled options. This is simpler
but requires the host to have already serialized the lowerdir list.

---

## 3. virtio-blk Variant: Host-Side Setup

When using the EROFS snapshotter (containerd 2.1+):

- **Per-layer EROFS files** live at
  `/var/lib/containerd/io.containerd.snapshotter.v1.erofs/snapshots/<ID>/layer.erofs`. These are
  read-only and shared across all containers using that image layer — no copy occurs.
- **Writable upper layer** is a fresh ext4 image file (or a thin-provisioned block device) created
  per container. It starts empty; CoW happens at the block level within the filesystem.
- **The kata runtime** attaches each layer file as a virtio-blk device to the VM. For
  a container with N image layers + 1 writable layer, the VM gets N+1 block devices hotplugged.
- **No full-image copy.** The EROFS layer files are the canonical stored form; the VM maps them
  read-only. The ext4 writable layer is created as an empty sparse file or thin provision.

For the older devicemapper path:
- The host containerd devicemapper snapshotter maintains a pool and creates a CoW thin provision
  per container. This block device is passed through as virtio-blk/scsi and mounted directly inside
  the VM — no in-guest overlayfs needed because the CoW is handled by device-mapper on the host.

---

## 4. Performance Lessons

Official cold-start numbers from Kata's own documentation are sparse. What is documented:

- **virtio-9p is severely slow** — acknowledged as "very slow, not fully POSIX compliant, and
  unstable." Abandoned as a primary path.
- **virtio-fs is the current default** for overlayfs snapshotters. DAX support in virtiofsd removes
  the need to copy data into guest memory; pages are mapped directly from the host page cache. This
  is the main perf win over 9p.
- **Devicemapper snapshotter gives the best I/O throughput** because the block device is passed
  through and the guest kernel's own filesystem code runs directly on it, without any shared-fs
  overhead. The trade-off is host-side complexity (dedicated dm partition, considered obsolete
  technology, containerd removed it).
- **EROFS + virtio-blk** is the current direction. The EROFS snapshotter (containerd 2.1+) improves
  image unpacking by ~14% vs gzip (measured with erofs-utils 1.8.2 on a WordPress image). The
  read-only layers are immutable and can be cached; only the upper layer needs I/O on writes. This
  closely matches what a copy-on-write snapshot provides.
- **DAX/pmem for the VM guest image** eliminates the cost of loading the VM's own rootfs into guest
  RAM. Pages are demand-faulted from the host file, not copied. This is a Firecracker-incompatible
  feature (QEMU/cloud-hypervisor only).

The key architectural lesson: **avoid any O(image-size) operation per container start**. Kata's
evolution has been a steady march away from copying toward read-only sharing at the block or
page-cache level.

---

## 5. Direct Portability to m80

### What we can lift

The `pivot_rootfs` function (`src/agent/rustjail/src/mount.rs:523`) is clean, dependency-light
Rust. Its only crate dependencies are `nix` (already in most Linux Rust stacks) and the `defer!`
macro (trivially replaced with a `scopeguard`). The logic — open both old/new root as O_DIRECTORY
FDs, fchdir to new root, `pivot_root(".", ".")`, fchdir back to old, make rslave, MNT_DETACH
unmount — is correct and maps directly to what m80's in-guest init (m80-guestd) needs to do after
assembling the overlayfs.

### What requires adaptation

The `handle_multi_layer_erofs_group` function (`src/agent/src/storage/multi_layer_erofs.rs`,
approximately lines 130–295) is very close to what m80 needs, but couples into:

- `protocols::agent::Storage` (Kata's protobuf-generated type) — replace with m80's own wire type
- `sandbox: Arc<Mutex<Sandbox>>` — Kata's global VM state; m80 has no equivalent, but the
  device-wait logic (`wait_and_mount_layer`, which polls uevents or resolves a PCI address to a
  `/dev/vdX` path) is the only part that touches it
- `kata_sys_util::mount::create_mount_destination` — a small utility that creates the target dir
  and handles the edge case of a file-type target vs a directory-type target; trivially replaced

The **core overlayfs assembly loop** (steps 1–6 in the sequence above) is pure `std::fs` +
`nix::mount` and can be adapted with minimal change. The structure m80 wants:

```
/run/m80/<vm-id>/overlay/
    upper/          <- mounted writable ext4 block device
        upper/      <- overlayfs upperdir (dir within the ext4)
        work/       <- overlayfs workdir  (dir within the ext4)
    lower-0/        <- mounted read-only base ext4/erofs block device
    lower-1/        <- (additional layers if needed)
rootfs/             <- overlayfs mount target, becomes container "/"
```

The sequence is: mount base (read-only) → mount writable layer → `mkdir upper work` inside writable
layer → `mount -t overlay -o upperdir=...,lowerdir=...,workdir=...` → `pivot_root`.

### What Kata does that m80 does not need

- EROFS format for read-only layers. m80's base is already ext4. A plain bind-mount in read-only
  mode is sufficient as a lowerdir; ext4 in read-only mode works as an overlayfs lowerdir. There is
  no requirement that lowerdirs be a specific filesystem type.
- Multi-layer stacking. m80 has one shared base + one writable overlay. Kata's N-layer EROFS
  stacking is for OCI image layer deduplication. m80 can use a single lowerdir.
- Confidential Containers path (image pull inside guest, CDH). Not relevant.

### Recommended m80 approach

1. **Host side**: Keep one shared read-only base ext4 (the current 256 MiB image). Present it to
   the VM as a read-only virtio-blk device. Per-VM writable layer: create an empty sparse ext4
   image (e.g. 512 MiB, sparse so it takes ~0 bytes on disk at creation). Present it as a second
   virtio-blk device.
2. **Guest side (m80-guestd)**: Mount base block device read-only. Mount writable ext4 device at a
   temp path. Create `upper/` and `work/` inside the writable mount. Assemble overlayfs with
   `lowerdir=<base-mount>,upperdir=<rw-mount>/upper,workdir=<rw-mount>/work` onto the rootfs path.
   Call `pivot_rootfs` (lifted from kata, ~40 lines of nix code).
3. **Teardown**: Unmount overlay first, then the two backing mounts (same ordering Kata uses via
   `temp_mount_points`).

This eliminates the 727 ms file copy entirely. The only per-VM cost is creating the sparse ext4
file (effectively a filesystem `mkfs` on a sparse file, sub-100 ms for a modern kernel) or
pre-allocating it at pool init time.

---

## Key Files Cited

| File | What it contains |
|---|---|
| `src/agent/rustjail/src/mount.rs:167` | `init_rootfs` — OCI spec mounts into rootfs dir |
| `src/agent/rustjail/src/mount.rs:523` | `pivot_rootfs` — fd-based pivot_root sequence |
| `src/agent/rustjail/src/mount.rs:665` | `ms_move_root` — MS_MOVE fallback when no_pivot |
| `src/agent/rustjail/src/container.rs:587` | `do_init_child` — orchestrates init_rootfs + pivot |
| `src/agent/src/storage/fs_handler.rs:1` | `OverlayfsHandler` — simple overlayfs storage path |
| `src/agent/src/storage/multi_layer_erofs.rs:130` | `handle_multi_layer_erofs_group` — EROFS + overlayfs assembly |
| `src/agent/src/storage/block_handler.rs:1` | `VirtioBlkMmioHandler`, `VirtioBlkPciHandler` — block device storage |
| `docs/design/architecture/storage.md` | Architecture overview: virtiofs vs devicemapper vs SCSI |

---

## Local-source deep dive (verified 2026-05-04)

All line references are to `/tank/projects/kata-containers` — full local checkout, not WebFetch
approximations.

---

### 1. `pivot_rootfs` — verbatim body

File: `src/agent/rustjail/src/mount.rs`, lines 507–559.

The module-level imports required by this function:

```rust
use nix::fcntl::{self, OFlag};
use nix::mount::{MntFlags, MsFlags, mount, umount2};
use nix::sys::stat::{self, Mode};
use nix::unistd;
use nix::NixPath;
use anyhow::{Context, Result};
// defer! comes from scopeguard, imported via `#[macro_use] extern crate scopeguard;`
// In our codebase: `use scopeguard::defer;` or add scopeguard to Cargo.toml and use
// the macro directly.  The macro closes a RawFd on scope exit.
```

The function and its private `pivot_root` shim (test vs production split):

```rust
// Lines 507–521: private shim allows unit tests to call pivot_rootfs without
// actually performing the syscall (pivot_root(2) requires a real mount namespace).
#[cfg(not(test))]
fn pivot_root<P1: ?Sized + NixPath, P2: ?Sized + NixPath>(
    new_root: &P1,
    put_old: &P2,
) -> anyhow::Result<(), nix::Error> {
    unistd::pivot_root(new_root, put_old)
}

#[cfg(test)]
fn pivot_root<P1: ?Sized + NixPath, P2: ?Sized + NixPath>(
    _new_root: &P1,
    _put_old: &P2,
) -> anyhow::Result<(), nix::Error> {
    Ok(())
}

// Lines 523–559: the public function.
pub fn pivot_rootfs<P: ?Sized + NixPath + std::fmt::Debug>(path: &P) -> Result<()> {
    let oldroot = fcntl::open("/", OFlag::O_DIRECTORY | OFlag::O_RDONLY, Mode::empty())?;
    defer!(unistd::close(oldroot).unwrap());
    let newroot = fcntl::open(path, OFlag::O_DIRECTORY | OFlag::O_RDONLY, Mode::empty())?;
    defer!(unistd::close(newroot).unwrap());

    // Change to the new root so that the pivot_root actually acts on it.
    unistd::fchdir(newroot)?;
    pivot_root(".", ".").context(format!("failed to pivot_root on {path:?}"))?;

    // Currently our "." is oldroot (according to the current kernel code).
    // However, purely for safety, we will fchdir(oldroot) since there isn't
    // really any guarantee from the kernel what /proc/self/cwd will be after a
    // pivot_root(2).
    unistd::fchdir(oldroot)?;

    // Make oldroot rslave to make sure our unmounts don't propagate to the
    // host. We don't use rprivate because this is known to cause issues due
    // to races where we still have a reference to a mount while a process in
    // the host namespace are trying to operate on something they think has no
    // mounts (devicemapper in particular).
    mount(
        Some("none"),
        ".",
        Some(""),
        MsFlags::MS_SLAVE | MsFlags::MS_REC,
        Some(""),
    )?;

    // Perform the unmount. MNT_DETACH allows us to unmount /proc/self/cwd.
    umount2(".", MntFlags::MNT_DETACH).context("failed to do umount2")?;

    // Switch back to our shiny new root.
    unistd::chdir("/")?;
    stat::umask(Mode::from_bits_truncate(0o022));
    Ok(())
}
```

**Ordering analysis (why each step is where it is):**

1. Both FDs opened before any `fchdir` — if either open fails the defer guards on the other still
   run cleanly.
2. `fchdir(newroot)` before `pivot_root(".", ".")` — the `"."` argument to `pivot_root` resolves
   relative to cwd; the kernel requires cwd to be inside the new root.
3. `pivot_root(".", ".")` with both args identical — runc's "no separate put_old mountpoint" trick.
   The kernel stacks old root under the new root momentarily; after the call, `/proc/self/cwd`
   points at the old root (per current kernel behavior, though the comment notes this isn't
   contractually guaranteed).
4. `fchdir(oldroot)` for safety — uses the pre-opened FD rather than relying on `/proc/self/cwd`.
5. `MS_SLAVE | MS_REC` on `"."` (which is old root) before `umount2` — prevents the detach
   propagating up the mount tree to the host. `MS_SLAVE` not `MS_PRIVATE` because private is known
   to race with devicemapper when the host still holds a reference to a mount.
6. `umount2(".", MNT_DETACH)` — detach rather than a normal unmount because `/proc/self/cwd` is
   still pointing at the directory being unmounted; `MNT_DETACH` queues the unmount until all
   references drop.
7. `chdir("/")` — now that old root is gone, move into the new root.
8. `umask(0o022)` — reset umask to a sane default after the namespace transition.

**Crate dependencies to lift this into m80-guestd:**

| Dependency | Cargo.toml key | Notes |
|---|---|---|
| `nix` | `nix = { version = "...", features = ["mount", "fcntl", "unistd", "stat"] }` | Already in most Linux Rust stacks |
| `scopeguard` | `scopeguard = "1"` | Provides `defer!`. Alternatively inline a `struct Defer<F: FnOnce()>` RAII guard — ~8 lines, zero deps |
| `anyhow` | `anyhow = "1"` | Binary edge only; use `thiserror` in library crates per m80 rules and convert at the call site |

The `pivot_root` shim pattern (cfg(not(test)) vs cfg(test)) is worth keeping for m80-guestd unit
tests, since `pivot_root(2)` requires a real mount namespace and will fail in a plain `cargo test`
run without root + `CLONE_NEWNS`.

---

### 2. `handle_multi_layer_erofs_group` — verified core sequence

File: `src/agent/src/storage/multi_layer_erofs.rs`, lines 101–322.

The copyright header reads `Copyright (c) 2026 Ant Group` — this is genuinely new code, not
present in any earlier WebFetch snapshot.

**Core mount sequence (lines 168–315), stripped of logging and bookkeeping:**

```rust
// lines 168–179: path traversal validation then temp-tree setup
let cid_str = cid.as_deref().unwrap_or("sandbox");
validate_container_id(cid_str)?;   // rejects "..", "/", null bytes, empty
let temp_base = PathBuf::from(format!("/run/kata-containers/{}/multi-layer", cid_str));
fs::create_dir_all(&temp_base)?;

validate_mount_point(&ext4.mount_point)?;  // same class of check on the final target

// lines 178–181: mount the writable ext4 upper layer into a temp subdir
let upper_mount = temp_base.join("upper");
fs::create_dir_all(&upper_mount)?;
wait_and_mount_layer(ext4, &upper_mount, sandbox, &logger).await?;

// lines 183–213: optional pre-overlay mkdir directives into the upper layer
// (X-kata.mkdir.path= options; not relevant to m80)

// lines 203–213: mount each EROFS read-only lower layer
let mut lower_mounts = Vec::new();
for (index, erofs) in erofs_storages.iter().enumerate() {
    let lower_mount = temp_base.join(format!("lower-{}", index));
    fs::create_dir_all(&lower_mount)?;
    wait_and_mount_layer(erofs, &lower_mount, sandbox, &logger).await?;
    lower_mounts.push(lower_mount);
}

// lines 238–244: upperdir and workdir live *inside* the mounted ext4 volume
let upperdir = upper_mount.join("upper");
let workdir  = upper_mount.join("work");
if !upperdir.exists() { fs::create_dir_all(&upperdir)?; }
fs::create_dir_all(&workdir)?;

// lines 246–250: build lowerdir= list, outermost layer first (index 0)
let lowerdir = lower_mounts.iter()
    .map(|p| p.display().to_string())
    .collect::<Vec<_>>()
    .join(":");

// lines 261–267: ensure target directory exists (kata_sys_util helper)
create_mount_destination(
    Path::new("overlay"),
    Path::new(&ext4.mount_point),
    "",
    "overlay",
)?;

// lines 269–284: the overlay mount itself
let overlay_options = format!(
    "upperdir={},lowerdir={},workdir={}",
    upperdir.display(), lowerdir, workdir.display()
);
baremount(
    Path::new("overlay"),
    Path::new(&ext4.mount_point),
    "overlay",
    nix::mount::MsFlags::empty(),
    &overlay_options,
    &logger,
)?;

// lines 311–315: return temp_mount_points (upper first, then lowers) for teardown
// overlay must be unmounted before these; the caller is responsible for that ordering
```

**`wait_and_mount_layer` (lines 465–551)** is the only function touching `Sandbox`. It resolves a
PCI path or SCSI address to a `/dev/vdX` device node by waiting for kernel uevents, then calls
`baremount`. The uevent wait is the sole reason `Sandbox` is threaded through — m80-guestd won't
need this because device paths will be known at boot time (fixed slots, not hotplug).

**`baremount` (src/agent/src/mount.rs, lines 67–120)** wraps `nix::mount::mount`. It checks that
source, destination, and fstype are non-empty, skips the mount if the destination is already
mounted with the same fstype (idempotency guard), then calls through. Self-contained; the only
non-std dependencies are `nix` and `slog` (logging). m80-guestd can substitute any logger or write
a thin wrapper.

---

### 3. Plain virtio-blk + ext4 path (no EROFS)

**WebFetch era finding:** "virtio-blk + ext4 base + ext4 overlay" was mentioned as an alternative.
**Local source finding:** There is no such path in the agent source. The two real paths are:

| Path | Handler | In-guest overlayfs? |
|---|---|---|
| `overlayfs` driver + `io.katacontainers.fs-opt.overlay-rw` option | `OverlayfsHandler` (`fs_handler.rs:22`) | Yes — host provides lowerdir list; agent adds `upper/` and `work/` and mounts overlay |
| `erofs.multi-layer` driver | `MultiLayerErofsHandler` + `handle_multi_layer_erofs_group` | Yes — agent mounts each block device and assembles overlay |
| `blk-pci` / `blk-mmio` driver (plain block) | `VirtioBlkPciHandler` / `VirtioBlkMmioHandler` (`block_handler.rs`) | No — device is mounted directly as the rootfs; CoW is handled host-side by devicemapper |

The "plain virtio-blk + ext4 + in-guest overlayfs" combination does not exist as a first-class
path. The VirtioBlkPciHandler mounts the block device directly at `storage.mount_point` via
`common_storage_handler`; there is no overlayfs assembly step. This is the devicemapper path: the
host thin-provision device already contains the writable merged view, so no in-guest overlay is
needed.

For m80, this is actually reassuring: **the path m80 needs (plain ext4 base + plain ext4 upper +
in-guest overlayfs assembly) does not exist in kata-agent**. We can implement it cleanly without
fighting kata's abstractions. The assembly logic is straightforward:

```
mount /dev/vda <base-mount> -t ext4 -o ro
mount /dev/vdb <upper-mount> -t ext4
mkdir -p <upper-mount>/upper <upper-mount>/work
mount -t overlay overlay <rootfs> \
  -o lowerdir=<base-mount>,upperdir=<upper-mount>/upper,workdir=<upper-mount>/work
pivot_rootfs(<rootfs>)
```

---

### 4. Tests

**Unit tests for `pivot_rootfs`:** `src/agent/rustjail/src/mount.rs:1269–1274`:

```rust
#[test]
#[serial(chdir)]
fn test_pivot_root() {
    let ret = pivot_rootfs("/tmp");
    assert!(ret.is_ok(), "Should pass. Got: {:?}", ret);
}
```

This works in `cargo test` only because the `#[cfg(test)]` shim stubs out the actual `pivot_root`
syscall (line 515–521). The test validates the FD-open, fchdir, MS_SLAVE mount, umount2, and
chdir steps against a real filesystem but without needing a mount namespace. `#[serial(chdir)]`
serializes all tests that mutate process cwd.

**Unit tests for `handle_multi_layer_erofs_group`:** `multi_layer_erofs.rs:554–730`. These cover
`validate_container_id`, `validate_mount_point`, `parse_mkdir_directive`,
`resolve_mkdir_path`, `is_upper_storage`, `is_lower_storage`, and `is_multi_layer_storage` — the
pure-logic helpers. There are **no integration tests** that actually mount devices and run the full
overlay assembly sequence; that would require root, block devices, and a mount namespace. Kata's
integration test suite for this lives in the Kubernetes-level bats tests, not in the Rust unit
test layer.

For m80: the same pattern applies. Unit-test the path-validation and option-assembly logic; the
mount sequence itself gets exercised by the VM integration tests.

---

### 5. License and attribution

Kata Containers is **Apache-2.0**. We can incorporate code verbatim with attribution.

For `pivot_rootfs` (the most likely verbatim lift):

```rust
// Adapted from kata-containers/src/agent/rustjail/src/mount.rs
// Copyright (c) 2019 Ant Financial
// SPDX-License-Identifier: Apache-2.0
// <https://github.com/kata-containers/kata-containers>
```

For the overlay assembly logic (likely adapted rather than verbatim):

```rust
// Adapted from kata-containers/src/agent/src/storage/multi_layer_erofs.rs
// Copyright (c) 2026 Ant Group
// SPDX-License-Identifier: Apache-2.0
```

The `baremount` helper, if lifted verbatim, carries the same `Copyright (c) 2019 Ant Financial`
header. In practice m80-guestd will want a simpler wrapper (no `slog`, no idempotency check) so
it's better to write fresh and not carry attribution for a ~5-line nix::mount::mount wrapper.

---

### 6. Adapter analysis for m80-guestd

**`pivot_rootfs` — extraction verdict: self-contained, lift verbatim.**

Dependencies that need resolution:

| Kata dependency | m80 resolution |
|---|---|
| `defer!(unistd::close(fd).unwrap())` | Add `scopeguard = "1"` to m80-guestd's Cargo.toml and use `#[macro_use] extern crate scopeguard;`, or write a 6-line `struct CloseOnDrop(RawFd)` |
| `nix::{fcntl, mount, unistd, sys::stat}` | `nix` is already in workspace deps |
| `anyhow::Context` | Binary-edge use in m80-guestd is fine |
| Module-local `pivot_root` shim | Replicate the cfg(not(test)) split to keep unit tests runnable without a mount namespace |

No Kata-specific types needed. The function signature `fn pivot_rootfs<P: ?Sized + NixPath + Debug>(path: &P) -> Result<()>` accepts any path type — `&str`, `&Path`, `&CStr` all implement `NixPath`.

**Overlay assembly — extraction verdict: write fresh, use kata as reference.**

The `handle_multi_layer_erofs_group` function's non-portable couplings:

| Coupling | What it does | m80 replacement |
|---|---|---|
| `protocols::agent::Storage` | Protobuf-generated type carrying source, fstype, options, mount_point | Define a plain `struct LayerSpec { source: PathBuf, fstype: String, flags: MsFlags, options: String, mount_point: PathBuf }` |
| `Arc<Mutex<Sandbox>>` | Passed to `wait_and_mount_layer` for uevent/hotplug device resolution | Not needed — m80 uses fixed block device slots known at boot; `source` is already `/dev/vda`, `/dev/vdb` |
| `kata_sys_util::mount::create_mount_destination` | Creates target dir, handles file-vs-dir edge case | `fs::create_dir_all(target)?` — m80 targets are always directories |
| `kata_sys_util::mount::parse_mount_options` | Parses option string into `(MsFlags, String)` | Write inline or pass flags + options directly |
| `slog::Logger` | Structured logging | m80-guestd's own logger (tracing or a simple eprintln for PID 1) |

The assembly loop itself — `create_dir_all` + `mount` for each layer, build lowerdir string, final
overlay mount — is ~30 lines of pure `std::fs` + `nix::mount` with no Kata-specific types in the
hot path. Write it fresh in m80-guestd as `fn mount_overlay(base: &Path, upper: &Path, target: &Path) -> Result<()>`.

**Teardown ordering** (from `MultiLayerErofsResult::temp_mount_points`, lines 311–315): overlay
target must be unmounted first, then upper, then each lower in order. This is a constraint m80 must
respect. Kata returns the temp_mount_points list and the caller registers them with the container
cleanup machinery; m80-guestd should do equivalent bookkeeping (e.g. store the mount sequence in a
`Vec<PathBuf>` and reverse it on teardown).

---

### Corrections to WebFetch-era summary

- Line reference for `handle_multi_layer_erofs_group`: WebFetch said "approximately lines 130–295";
  actual is lines 101–322 (the function grew; the copyright year 2026 confirms it was added/expanded
  recently).
- The "simple overlayfs + virtio-blk ext4" path described in section 3 of the original doc does not
  exist as a distinct in-guest code path. `OverlayfsHandler` handles the case where the host has
  already resolved lowerdirs (via virtiofs sharing the host overlay dirs); `VirtioBlkPciHandler`
  handles plain block devices without any in-guest overlay assembly. There is no "ext4 base + ext4
  upper + in-guest overlay" handler — m80 will be implementing novel territory there.
- `defer!` is from the `scopeguard` crate (re-exported via `#[macro_use] extern crate scopeguard`
  in `rustjail/src/lib.rs:15`), not a standalone `defer` crate. The original doc said "trivially
  replaced with a `scopeguard`" — that is exactly correct; it already is scopeguard.
