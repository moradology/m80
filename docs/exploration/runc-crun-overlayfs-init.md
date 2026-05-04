# runc / crun overlayfs + pivot_root reference

Research into how runc and crun implement the overlayfs + pivot_root sequence for
container PID 1. The goal is a near-liftable reference for m80-guestd's `pid_one.rs`.

## 1. Where in runc is the pivot_root performed?

runc's init sequence lives entirely in `libcontainer/`. The relevant call chain:

```
standard_init_linux.go  linuxStandardInit::Init()
  → rootfs_linux.go     prepareRootfs()
      → prepareRoot()         // propagation setup + bind-mount rootfs onto itself
      → setupAndMountToRootfs()  // per-mount: /proc, /sys, /dev, user mounts
      → pivotRoot()           // the actual pivot
```

**`libcontainer/rootfs_linux.go` — `prepareRoot` (lines 1101–1116)**

```go
func prepareRoot(config *configs.Config) error {
    flag := unix.MS_SLAVE | unix.MS_REC   // default; drops shared propagation
    if config.RootPropagation != 0 {
        flag = config.RootPropagation
    }
    // (A) Make the entire host mount tree slave/private so our mounts don't leak out.
    mount("", "/", "", flag, "")

    // (B) Walk up the rootfs path until we find a mount point; make it private.
    rootfsParentMountPropagation(config.Rootfs, config.RootPropagation)

    // (C) Bind-mount rootfs onto itself. This gives pivot_root a mount to
    //     work with — pivot_root(".", ".") requires the new root to itself
    //     be a mount point.
    mount(config.Rootfs, config.Rootfs, "bind", unix.MS_BIND|unix.MS_REC, "")
}
```

**`libcontainer/rootfs_linux.go` — `pivotRoot` (lines 1143–1202)**

```go
func pivotRoot(root *os.File) error {
    oldroot, _ := linux.Open("/", O_DIRECTORY|O_RDONLY|O_PATH, 0)
    defer unix.Close(oldroot)

    // cd into new root so pivot_root(".", ".") works.
    unix.Fchdir(int(root.Fd()))

    // pivot_root(".", ".") — no put_old directory needed.
    unix.PivotRoot(".", ".")

    // After pivot, cwd is the old root. fchdir via the saved fd for safety.
    unix.Fchdir(oldroot)

    // Mark old root as rslave before unmounting to prevent propagation to host.
    mount("", ".", "", unix.MS_SLAVE|unix.MS_REC, "")

    // Detach old root. MNT_DETACH unmounts even if cwd is under it.
    unmount(".", unix.MNT_DETACH)

    // Back into new root.
    unix.Chdir("/")
}
```

Note: runc's `pivotRoot` uses `MS_SLAVE|MS_REC` (not `MS_PRIVATE|MS_REC`) on the
old root before unmounting. The comment in the source explains this explicitly:
"We don't use rprivate because this is known to cause issues due to races where we
still have a reference to a mount while a process in the host namespace is trying
to operate on something they think has no mounts (devicemapper in particular)."

**Mounts happen before pivot**: `prepareRootfs` calls `setupAndMountToRootfs` for
every configured mount (including `/proc`, `/sys`, `/dev`) into the *future* rootfs
path while still on the host. Only after all mounts succeed does it call
`pivotRoot`. So `/proc` is mounted into `rootfs/proc` before the pivot, not after.

## 2. crun's `do_pivot` (src/libcrun/linux.c, lines 1953–2003)

crun's approach is identical in principle but more explicit about looping on
`umount2` until EINVAL:

```c
static int
do_pivot (libcrun_container_t *container, const char *rootfs, libcrun_error_t *err)
{
    oldrootfd = open ("/", O_DIRECTORY | O_PATH | O_CLOEXEC);
    newrootfd = open (rootfs, O_DIRECTORY | O_PATH | O_CLOEXEC);

    fchdir (newrootfd);
    pivot_root (".", ".");
    fchdir (oldrootfd);

    // Make old root private so unmounts don't propagate.
    do_mount (container, NULL, -1, ".", NULL, MS_REC | MS_PRIVATE, ...);

    // First detach.
    umount2 (".", MNT_DETACH);

    // Drain any submounts that survived the first detach.
    do {
        ret = umount2 (".", MNT_DETACH);
        if (ret < 0 && errno == EINVAL)
            break;    // EINVAL = nothing left to unmount
    } while (ret == 0);

    chdir ("/");
}
```

crun uses `MS_PRIVATE` on old root vs runc's `MS_SLAVE` — both work; the
MS_PRIVATE choice is simpler in crun's single-tenant context.

crun's `libcrun_set_mounts` (lines 2784–2810) does the same pre-pivot dance:
```c
do_mount("/", MS_REC | MS_PRIVATE)       // (A) drop host sharing
make_parent_mount_private(rootfs)         // (B) walk up to find mount point, make private
do_mount(rootfs, rootfs, MS_BIND|MS_REC|MS_PRIVATE)  // (C) bind rootfs onto itself
// ... then all the per-mount points ...
libcrun_do_pivot_root(...)
```

## 3. Tricky bits

### MS_PRIVATE before pivot_root — required, not optional

Both runtimes make the host mount tree `MS_SLAVE|MS_REC` (or `MS_PRIVATE|MS_REC`)
at the start of the init sequence. This is the gotcha: if any ancestor mount is
`MS_SHARED`, `pivot_root(2)` will fail with `EINVAL`. The kernel requires the
new-root mount and its parent to be private or slave before pivot. The bind-mount
of rootfs onto itself (step C above) also creates the required separate mount
entry.

### /proc before pivot, not after

runc and crun both mount `/proc` (and `/sys`, `/dev`) *into* the future rootfs
directory tree while the host's old root is still active. After pivot, `/proc` is
already there. In m80-guestd, the current approach of mounting `/proc` as the very
first act of PID 1 is fine — but it must be mounted at the *future* `/proc` path
(i.e. into the overlayfs merged root), not at the host initramfs `/proc`.

### Old root: unmount, not shadow

Both runtimes unmount the old root with `MNT_DETACH` rather than leaving it
shadowed. crun loops until `EINVAL` to drain all submounts. runc uses a single
`MNT_DETACH` on `.` (the cwd, which is the old root after `fchdir(oldroot)`).
The `pivot_root(".", ".")` trick puts the old root at `.` so no named `put_old`
directory is needed in the new root.

### workdir setup for overlayfs

From the kernel docs: the `workdir` must be an empty directory on the **same
filesystem** as `upperdir`. Both must be on a filesystem that supports
`trusted.*` xattrs and reports valid `d_type` in readdir. ext4 satisfies both
requirements without any special configuration. The caller must create `workdir`
before issuing the mount — the kernel does not create it.

## 4. Recommended sequence for m80-guestd

m80-guestd is PID 1 in a fresh initramfs with no existing mounts other than what
the kernel provides. The sequence below targets our specific setup:
- `/dev/vda` — read-only base ext4 (shared image)
- `/dev/vdb` — per-VM sparse ext4 (upper + work)
- `/dev/vdc` — workspace ext4

```rust
// ── Phase 1: mount the two layer disks ────────────────────────────────────
// The merged root does not yet exist; we build it from scratch.

// Base image (read-only).
fs::create_dir_all("/lower")?;
nix::mount::mount(
    Some("/dev/vda"), "/lower",
    Some("ext4"), MsFlags::MS_RDONLY, None::<&str>,
)?;

// Per-VM overlay disk (upper + workdir live here).
fs::create_dir_all("/upper")?;
nix::mount::mount(
    Some("/dev/vdb"), "/upper",
    Some("ext4"), MsFlags::empty(), None::<&str>,
)?;

// ── Phase 2: prepare overlayfs directories ────────────────────────────────
// workdir MUST be on the same filesystem as upperdir (both on /dev/vdb here).
// workdir MUST be empty; kernel will error if it is not.
fs::create_dir_all("/upper/root")?;   // the actual upper layer contents
fs::create_dir_all("/upper/.work")?;  // must be empty, same fs as upper

// Merged root mount point (on the initramfs tmpfs, not on either disk).
fs::create_dir_all("/merged")?;

// ── Phase 3: mount overlayfs ──────────────────────────────────────────────
let opts = "lowerdir=/lower,upperdir=/upper/root,workdir=/upper/.work";
nix::mount::mount(
    Some("overlay"), "/merged",
    Some("overlay"), MsFlags::empty(), Some(opts),
)?;

// ── Phase 4: populate /proc, /sys, /dev inside merged root ───────────────
// Do this BEFORE pivot so the dirs exist and mounts are in place on entry.
for dir in &["/merged/proc", "/merged/sys", "/merged/dev"] {
    fs::create_dir_all(dir)?;
}
nix::mount::mount(
    Some("proc"), "/merged/proc",
    Some("proc"), MsFlags::MS_NOEXEC | MsFlags::MS_NOSUID | MsFlags::MS_NODEV,
    None::<&str>,
)?;
nix::mount::mount(
    Some("sysfs"), "/merged/sys",
    Some("sysfs"), MsFlags::MS_NOEXEC | MsFlags::MS_NOSUID | MsFlags::MS_NODEV,
    None::<&str>,
)?;
nix::mount::mount(
    Some("devtmpfs"), "/merged/dev",
    Some("devtmpfs"), MsFlags::MS_NOSUID | MsFlags::MS_STRICTATIME,
    Some("mode=755"),
)?;

// ── Phase 5: propagation — must happen before pivot_root ─────────────────
// Make entire host tree slave so nothing leaks back to the host namespace.
// pivot_root(2) requires the new root's parent mount to not be MS_SHARED.
nix::mount::mount(
    None::<&str>, "/", None::<&str>,
    MsFlags::MS_SLAVE | MsFlags::MS_REC, None::<&str>,
)?;

// Bind-mount the merged root onto itself to give it a standalone mount entry.
// pivot_root(".", ".") requires new_root to be a mount point distinct from
// its parent.
nix::mount::mount(
    Some("/merged"), "/merged",
    None::<&str>, MsFlags::MS_BIND | MsFlags::MS_REC, None::<&str>,
)?;

// ── Phase 6: pivot_root(".", ".") ─────────────────────────────────────────
let old_root = nix::fcntl::open("/", OFlag::O_DIRECTORY | OFlag::O_PATH | OFlag::O_CLOEXEC, Mode::empty())?;
nix::unistd::chdir("/merged")?;
nix::unistd::pivot_root(".", ".")?;

// After pivot: cwd == old root. Use saved fd for safety.
nix::unistd::fchdir(old_root)?;

// Mark old root private/slave before detaching.
nix::mount::mount(
    None::<&str>, ".", None::<&str>,
    MsFlags::MS_SLAVE | MsFlags::MS_REC, None::<&str>,
)?;
nix::mount::umount2(".", MntFlags::MNT_DETACH)?;

// Back into the new root.
nix::unistd::chdir("/")?;
nix::unistd::close(old_root)?;

// ── Phase 7: mount workspace inside new root ──────────────────────────────
// /workspace must exist in the merged rootfs (either from /lower or written
// into /upper/root during image build).
nix::mount::mount(
    Some("/dev/vdc"), "/workspace",
    Some("ext4"), MsFlags::empty(), None::<&str>,
)?;

// ── Phase 8: proceed to vsock listener ───────────────────────────────────
```

## 5. ext4 upper disk — fresh format requirements

A freshly `mkfs.ext4`-formatted `/dev/vdb` requires only two things before use
as an overlayfs upper:

1. **Create the directories manually** — `upper/root/` and `upper/.work/`. The
   kernel will not create them. `workdir` must be empty at mount time; the kernel
   returns `EINVAL` if it contains anything.

2. **Extended attributes must be enabled** — ext4 enables xattrs by default since
   kernel 2.6.x; `mkfs.ext4` does not require any special flag. The overlayfs
   kernel module uses `trusted.overlay.*` xattrs to store whiteout markers and
   opaque directory flags. These will be written automatically on first copy-up.

No special `tune2fs` incantations, no `user_xattr` mount option (that is for the
`user.*` namespace; `trusted.*` is always available to root), no `dir_index`
tuning. A stock `mkfs.ext4 /dev/vdb && mount /dev/vdb /upper && mkdir
/upper/root /upper/.work` is all that is needed.

One subtle point: the workdir (`/upper/.work`) is an internal scratch area. After
mount, the kernel creates a `work/` subdirectory inside it. Do not write anything
into `.work` yourself; treat it as opaque after the overlayfs mount succeeds.

## 6. Key sources

| File | Lines | What it shows |
|---|---|---|
| `libcontainer/rootfs_linux.go` | 1101–1116 | `prepareRoot`: MS_SLAVE + bind-mount-self before pivot |
| `libcontainer/rootfs_linux.go` | 1143–1202 | `pivotRoot`: `fchdir` + `pivot_root(".",".") ` + `MS_SLAVE|MNT_DETACH` |
| `libcontainer/rootfs_linux.go` | 154–240 | `prepareRootfs`: mounts happen before pivot |
| `libcontainer/rootfs_linux.go` | 610–643 | proc/sysfs special-cased before other mounts |
| `src/libcrun/linux.c` | 1953–2003 | `do_pivot`: C equivalent, loop-drain submounts |
| `src/libcrun/linux.c` | 2783–2810 | `libcrun_set_mounts`: same three-step pre-pivot dance |
| `src/libcrun/linux.c` | 2983–3020 | `libcrun_do_pivot_root`: dispatch to `do_pivot` or chroot |
