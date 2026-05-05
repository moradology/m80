# Stripped Guest Kernel — Design (`m80-ci9i`)

**Cross-epic constraint (non-negotiable):** the keep-list below MUST include
`CONFIG_OVERLAY_FS=y` and `CONFIG_OVERLAY_FS_XINO_AUTO=y`. Omitting either
symbol breaks the overlay-rootfs mount in `m80-f2zc.4` and leaves the storage
pivot entirely non-functional. This dependency is tracked as
`m80-f2zc.5 → m80-ci9i.1` (related-dep).

---

## 1. Kernel version pin

Pin to **Linux 6.1.x LTS** — specifically the latest stable point release at
build time (currently `v6.1.134`, but resolved at Dockerfile build time by
the `linux-stable` mirror rather than hard-coded).

Rationale:

- 6.1 is the current Long-Term Stable series (EOL Dec 2026), maintained
  actively for security fixes.
- Firecracker's own CI tracks 5.10.x and 6.1.x; 6.1 gives us the wider
  test surface. The firecracker-ci kernel bucket ships a 5.10.245 vmlinux;
  we are not constrained to match that version because we own the kernel
  build.
- 6.1 carries all the vsock and virtio-mmio fixes that 5.10 accumulated as
  back-ports; choosing 6.1 avoids having to cherry-pick them.
- `CONFIG_OVERLAY_FS_XINO_AUTO` was introduced in 5.15; both 6.1 and 5.15
  carry it. 5.10 does not; this alone disqualifies 5.10 without a patch.

The exact commit is pinned by the Dockerfile via `KERNEL_TAG` build-arg
(default `v6.1.134`). Changing the pin requires bumping `KERNEL_TAG` in the
Dockerfile and re-running the build pipeline; the resulting kernel sha is
captured in the manifest.

---

## 2. CONFIG keep list

Every symbol below must appear as `=y` (built-in, not `=m`) in the stripped
kernel. Module support (`CONFIG_MODULES`) is off; there is no module loader in
the guest rootfs.

### Core VM communication

| Symbol | Purpose |
|---|---|
| `CONFIG_VSOCKETS` | AF_VSOCK socket family |
| `CONFIG_VIRTIO_VSOCKETS` | virtio vsock transport |
| `CONFIG_VIRTIO_VSOCKETS_COMMON` | shared vsock helpers |

### Block and network virtio

| Symbol | Purpose |
|---|---|
| `CONFIG_VIRTIO` | virtio core |
| `CONFIG_VIRTIO_MMIO` | MMIO transport (replaces PCI; see drop list) |
| `CONFIG_VIRTIO_BLK` | virtio block (rootfs, overlay, scratch drives) |
| `CONFIG_VIRTIO_NET` | virtio network (OutboundNat mode; v0.2) |
| `CONFIG_BLK_MQ_VIRTIO` | multi-queue block layer integration |

### Filesystems

| Symbol | Purpose |
|---|---|
| `CONFIG_EXT4_FS` | ext4 rootfs and overlay disk |
| `CONFIG_OVERLAY_FS` | overlayfs for storage-pivot (m80-f2zc) |
| `CONFIG_OVERLAY_FS_XINO_AUTO` | auto inode number remapping; required by m80-f2zc.5 — non-negotiable |

### Device management

| Symbol | Purpose |
|---|---|
| `CONFIG_DEVTMPFS` | auto-populate /dev at boot |
| `CONFIG_DEVTMPFS_MOUNT` | mount devtmpfs automatically |

### Console and diagnostics

| Symbol | Purpose |
|---|---|
| `CONFIG_SERIAL_8250` | 8250/16550 UART driver |
| `CONFIG_SERIAL_8250_CONSOLE` | boot console on ttyS0 |
| `CONFIG_PRINTK` | kernel message ring buffer (essential for any boot-stage diagnostics) |

### Entropy (required by guestd crypto paths)

| Symbol | Purpose |
|---|---|
| `CONFIG_HW_RANDOM_VIRTIO` | virtio RNG; feeds /dev/random early |
| `CONFIG_RANDOM_TRUST_CPU` | accept RDRAND entropy at boot — reduces entropy starvation stall |

### Process and memory basics

| Symbol | Purpose |
|---|---|
| `CONFIG_BINFMT_ELF` | ELF binary loader (guestd is an ELF) |
| `CONFIG_TMPFS` | used by devtmpfs and PID-1 /proc setup |
| `CONFIG_PROC_FS` | /proc; guestd reads /proc/mounts |
| `CONFIG_SYSFS` | /sys |
| `CONFIG_NET` | networking subsystem (required even for vsock-only) |
| `CONFIG_INET` | IPv4 (required for loopback; OutboundNat v0.2) |
| `CONFIG_UNIX` | AF_UNIX (not used inside guest but linked by vsock init path) |

---

## 3. CONFIG drop list

These are explicitly off (`=n` or absent) in the stripped config. The goal is
to reduce decompressed kernel size and eliminate init-time driver probe loops
that add latency to `phase_12b_ready_accept`.

### Transport and bus

| Symbol | Rationale |
|---|---|
| `CONFIG_PCI` | disabled; all devices use virtio-mmio |
| `CONFIG_PCI_MSI` | follows PCI=n |
| `CONFIG_PCCARD` | PC Card / PCMCIA — irrelevant |
| `CONFIG_AGP` | AGP bus — irrelevant |
| `CONFIG_ACPI` | Firecracker does not present ACPI; disabling cuts init time |
| `CONFIG_DMI` | Desktop Management Interface — unused in VMs |
| `CONFIG_EFI` | Firecracker boots via legacy BIOS mode only |

### Filesystems (beyond the keep list)

`CONFIG_BTRFS_FS`, `CONFIG_XFS_FS`, `CONFIG_F2FS_FS`, `CONFIG_NILFS2_FS`,
`CONFIG_REISERFS_FS`, `CONFIG_JFS_FS`, `CONFIG_GFS2_FS`, `CONFIG_OCFS2_FS`,
`CONFIG_NFS_FS`, `CONFIG_NFSD`, `CONFIG_CIFS`, `CONFIG_SMB_SERVER`,
`CONFIG_FUSE_FS`, `CONFIG_SQUASHFS`, `CONFIG_CRAMFS`, `CONFIG_ROMFS_FS`,
`CONFIG_ISO9660_FS`, `CONFIG_UDF_FS`, `CONFIG_VFAT_FS`, `CONFIG_NTFS3_FS`,
`CONFIG_HFS_FS`, `CONFIG_HFSPLUS_FS`.

Exception: `CONFIG_TMPFS=y` (kept; see above).

### Network protocols (beyond IPv4)

`CONFIG_IPV6`, `CONFIG_BRIDGE`, `CONFIG_VLAN_8021Q`, `CONFIG_ATALK`,
`CONFIG_DECNET`, `CONFIG_TIPC`, `CONFIG_RDS`, `CONFIG_SCTP`, `CONFIG_DCCP`,
`CONFIG_L2TP`, `CONFIG_PPTP`, `CONFIG_WIRELESS`, `CONFIG_CFG80211`,
`CONFIG_MAC80211`, `CONFIG_NFC`, `CONFIG_BLUETOOTH`.

### Hardware drivers

**USB:** `CONFIG_USB`, `CONFIG_USB_SUPPORT` and all subsystems under it.

**Sound:** `CONFIG_SOUND`, `CONFIG_SND` and all ALSA/ASoC drivers.

**Graphics:** `CONFIG_DRM`, `CONFIG_FB` (framebuffer), `CONFIG_VGA_CONSOLE`,
`CONFIG_LOGO`.

**Input devices:** `CONFIG_INPUT`, `CONFIG_HID`, `CONFIG_HID_GENERIC`,
`CONFIG_USB_HID`.

**Storage controllers not used:** `CONFIG_ATA`, `CONFIG_SCSI_LOWLEVEL`,
`CONFIG_RAID6_PQ`, `CONFIG_MD` (software RAID), `CONFIG_DM` (device-mapper —
not needed since we don't use LVM or dm-verity).

**Platform and IPMI:** `CONFIG_IPMI_HANDLER`, `CONFIG_SENSORS_*`,
`CONFIG_HWMON`, `CONFIG_WATCHDOG`, `CONFIG_RTC_CLASS`.

### Security modules

`CONFIG_SECURITY_SELINUX`, `CONFIG_SECURITY_APPARMOR`,
`CONFIG_SECURITY_TOMOYO`, `CONFIG_SECURITY_SMACK`, `CONFIG_IMA`,
`CONFIG_EVM`. Keep `CONFIG_SECURITY=y` (the framework) but with no mandatory
access control policy loaded. The guest is single-workload and
single-tenant; MAC adds latency with no benefit.

### Debugging and tracing

`CONFIG_DEBUG_KERNEL`, `CONFIG_KGDB`, `CONFIG_KDB`, `CONFIG_KPROBES`,
`CONFIG_FTRACE`, `CONFIG_TRACING`, `CONFIG_PERF_EVENTS`,
`CONFIG_DEBUG_FS` (debugfs), `CONFIG_PROFILING`, `CONFIG_OPROFILE`,
`CONFIG_CRASH_DUMP` (kdump), `CONFIG_KEXEC`.

Keep `CONFIG_PRINTK=y` — this is in the keep list and is non-negotiable for
boot diagnostics.

### Misc

`CONFIG_MODULES` (no loadable module support), `CONFIG_KALLSYMS`,
`CONFIG_PROC_KCORE`, `CONFIG_SWAP`, `CONFIG_HIBERNATION`, `CONFIG_SUSPEND`,
`CONFIG_PM`, `CONFIG_EXPERT` (enables some of the above when set; fine to
leave unset).

---

## 4. Build environment

The kernel is built in a Docker container to ensure reproducibility and
isolate host toolchain variation. The Dockerfile path, established by this
design, is:

```
crates/m80-image-build/kernel-builder/Dockerfile
```

The file is written in IMPL leaf `m80-ci9i.2`.

### Container spec

- **Base:** `ubuntu:22.04` pinned by digest (not tag), so
  `FROM ubuntu@sha256:<digest>` — resolved once at Dockerfile authoring time
  and recorded in the file. Prevents silent toolchain drift.
- **Packages:** `build-essential bc flex bison libelf-dev libssl-dev
  libncurses-dev wget ca-certificates`
- **No network access at build time**: the kernel source tarball and config
  are COPY'd into the container; the container performs only the build step.
- **Determinism pins:**
  - `KBUILD_BUILD_TIMESTAMP=0` — strips timestamps from the vmlinux ELF.
  - `SOURCE_DATE_EPOCH=0` — aligns any date-embedding in helper tools.
  - `KBUILD_BUILD_USER=m80` and `KBUILD_BUILD_HOST=m80-builder` — fixed
    strings so the embedded ident string is reproducible.
- **Output:** `/out/vmlinux` inside the container; the Makefile target is
  `vmlinux` only (no modules, no `bzImage` — Firecracker loads `vmlinux`
  directly).

Build invocation (host side):

```sh
docker build -t m80-kernel-builder crates/m80-image-build/kernel-builder/
docker run --rm \
  -v "$(pwd)/crates/m80-image-build/kernel-builder/m80-6.1.config:/config:ro" \
  -v "$(pwd)/crates/m80-image-build/kernels:/out" \
  m80-kernel-builder
```

The `.config` file is the committed stripped config; the Dockerfile copies it
into the kernel tree before `make olddefconfig && make vmlinux`.

Acceptance: given the same `KERNEL_TAG` and `.config`, two independent
builds produce byte-identical `vmlinux` output (verified by sha256 comparison
in `m80-ci9i.2` IMPL acceptance).

---

## 5. Output convention

### File path

```
crates/m80-image-build/kernels/vmlinux-m80-<config-sha>.bin
```

`<config-sha>` is the lowercase hex sha256 of the `.config` file used to
build the kernel — not the sha256 of the vmlinux itself. This keys the
artifact on the build inputs, not the outputs, so two builds from the same
config produce the same filename.

The `kernels/` directory is gitignored (binary blobs); only the `.config`
source and Dockerfile are committed.

### Manifest schema extension

`m80-image-manifest` gains a `kernel_kind` field on `Manifest`:

```rust
/// Which kernel was used to boot this image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KernelKind {
    /// Upstream Firecracker CI kernel (downloaded from the S3 bucket).
    Stock,
    /// Purpose-built stripped kernel (built via m80-ci9i.2 pipeline).
    Stripped,
}
```

The `Manifest` struct gains:

```rust
pub kernel_kind: KernelKind,
```

This field is required (not `Option`); existing Stock manifests must be
rebuilt with `kernel_kind: "stock"` once the schema version bumps.
`SCHEMA_VERSION` increments to `3` when this field lands (in `m80-ci9i.2`
IMPL, not this DESIGN leaf — no code in DESIGN).

The `boot_args_for` dispatch in `m80-firecracker/src/launch.rs` will gain a
third dispatch axis on `KernelKind::Stripped` (cmdline trim; see §6 below),
wired in `m80-ci9i.3`.

---

## 6. Cmdline trim spec

### Trade-off decision: `8250.nr_uarts=1` vs `=0`

Setting `8250.nr_uarts=0` saves approximately 50 ms by skipping all UART
probe loops. This is rejected. Per CLAUDE.md "diagnostics before hypotheses":
console output is the first tool when a boot fails or hangs. Suppressing it
to save 50 ms trades a concrete diagnostic capability for a marginal latency
gain. The 500-700 ms expected save from the stripped kernel is not contingent
on this 50 ms; the remaining chain reaches the sub-200 ms warm-pool target
without it.

**Decision: `8250.nr_uarts=1` — one UART, console preserved.**

### Final cmdline for `KernelKind::Stripped`

```
console=ttyS0 reboot=k panic=-1 quiet loglevel=0 8250.nr_uarts=1
```

Diff from the current `COMMON_BOOT_ARGS` (`console=ttyS0 reboot=k panic=-1 pci=off`):

- `pci=off` → **removed**. With `CONFIG_PCI=n` in the kernel, this flag is
  a no-op and adds nothing; removing it keeps the cmdline honest.
- `quiet loglevel=0` → **added**. Suppresses per-device init messages on
  ttyS0 while leaving the console open. Fatal panics still print (the panic
  handler bypasses loglevel). Saves ~20-40 ms of serial flush time on boot.
- `8250.nr_uarts=1` → **added**. Explicit single-UART cap; prevents probe
  of the four default UARTs on driver init.

### `init=` for Stripped + Minimal combination

When `image_kind=Minimal` and `kernel_kind=Stripped`, the kernel resolves
`/init -> /m80-guestd` via the symlink the Minimal pipeline bakes in
(step 6, `m80-image-build/src/minimal.rs`). The explicit `init=/m80-guestd`
from the current `Minimal` boot-args path is retained as a belt-and-suspenders;
the symlink is the backstop. No change needed to `boot_args_for` for this
combination.

For `image_kind=Ubuntu` and `kernel_kind=Stripped`, systemd is the init; no
`init=` override.

The `boot_args_for` function in `m80-ci9i.3` will handle the two-axis
dispatch:

```
(Ubuntu, Stock)   → COMMON_BOOT_ARGS
(Ubuntu, Stripped)→ STRIPPED_BOOT_ARGS
(Minimal, Stock)  → COMMON_BOOT_ARGS + " init=/m80-guestd"
(Minimal, Stripped)→ STRIPPED_BOOT_ARGS + " init=/m80-guestd"
```

---

## 7. Risk register

Distilled from `docs/planning/perf-roadmap-extended.md §2.1`.

| ID | Failure mode | Detection | Mitigation |
|---|---|---|---|
| R1 | Stripped kernel lacks `CONFIG_OVERLAY_FS=y` — overlay mount fails, `phase_12b_ready_accept` times out | guest panics; phase times out at 60 s | Keep-list (§2 above) includes both overlayfs symbols as non-negotiable. Smoke checkpoint (`m80-ci9i.3b`) asserts `mount` shows overlayfs at `/` on first boot. |
| R2 | A stripped CONFIG drops a feature guestd depends on (e.g. AES-NI path through kernel crypto, HW-RNG starvation) | guestd panics or hangs on first crypto op | Smoke checkpoint (`m80-ci9i.3b`) runs `sha256sum /etc/os-release` inside guest and asserts correct output. `CONFIG_HW_RANDOM_VIRTIO=y` is in the keep-list. |
| R3 | Boot latency save is less than expected (saves 200 ms instead of 500-700 ms) | BENCH (`m80-ci9i.4`) shows < 500 ms improvement | No mitigation — accept the data. The cumulative chain (storage pivot + snapshot/restore) still reaches sub-200 ms warm-pool target even with conservative kernel save. Document the actual number. |
| R4 | Stripped kernel boots fine on test host (x86_64 Ryzen) but flakes on production host (AWS c5.metal) | Out-of-tree user report or CI on a different instance type | Document the test platform in this DESIGN. Commit to "boot-tested on x86_64 with KVM". Multi-arch and multi-platform support is a v0.3 concern per existing non-goals. |
| R5 | Build environment drifts; vmlinux sha changes across builds without code changes | Manifest sha mismatch surprises developer | `m80-ci9i.2` build is deterministic: Ubuntu 22.04 pinned by digest, `KBUILD_BUILD_TIMESTAMP=0`, `SOURCE_DATE_EPOCH=0`, fixed `KBUILD_BUILD_USER`/`HOST`. Same input twice must produce byte-identical vmlinux. |
| R6 | Custom kernel receives a security CVE and the tree falls behind upstream LTS | Slow-burn CVE accumulation | Out of scope for v0.2. `m80-ci9i.5` DOCS records this explicit deferral: "kernel security hardening (CVE tracking, config audit) is deferred to v0.3." The 6.1 LTS track receives upstream security fixes; consuming them requires re-running the build pipeline, which is the operator's responsibility. |
| R7 | `8250.nr_uarts=0` would suppress console and destroy boot-stage diagnostics | When anything fails post-strip, no output on ttyS0 | **Decided:** pin `nr_uarts=1` (see §6). The 50 ms saving from `=0` is not worth the diagnostic loss. This is a locked decision; not a mitigation. |

---

## 8. Cross-epic constraint summary

The following constraint must survive into every future edit of this document
and of the kernel `.config`:

`CONFIG_OVERLAY_FS=y` and `CONFIG_OVERLAY_FS_XINO_AUTO=y` are required
by the storage-pivot epic (`m80-f2zc.5`). If either symbol is absent from
the stripped kernel, `m80-f2zc.4`'s guest-side overlay mount fails and the
727 ms storage-pivot saving is entirely lost — not degraded, lost. The
dependency is tracked as `m80-f2zc.5 → m80-ci9i.1` (related-dep) in the
bead graph.

**Empirical finding (2026-05-04):** The stock firecracker-ci kernel at
`/tmp/m80-build/minimal/vmlinux` was probed via `extract-ikconfig`:

- `CONFIG_OVERLAY_FS=y` — **confirmed present** in the stock kernel.
- `CONFIG_OVERLAY_FS_XINO_AUTO` — **NOT SET** in the stock kernel (the line
  reads `# CONFIG_OVERLAY_FS_XINO_AUTO is not set`).

This means `m80-f2zc.5` would catch `XINO_AUTO` missing if it verifies both
symbols, and the stripped kernel is the vehicle that brings `XINO_AUTO` into
the build. The stripped kernel must not replicate the stock kernel's omission.
