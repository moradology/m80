# Storage pivot: bead plan

**Date:** 2026-05-04
**Author of plan:** storage-pivot exploration round
**Status:** proposed; beads not yet created
**Companion docs:** `docs/exploration/{kata-containers,firecracker-shared-rootfs,overlayfs-kernel-semantics,runc-crun-overlayfs-init,firecracker-containerd,cloud-hypervisor}.md`

## Executive summary

- **Pivot:** delete `m80-storage::Rootfs::clone` (727 ms full file-copy of the 256 MiB base ext4) in favour of one shared read-only base host file + a per-VM sparse ext4 overlay + in-guest overlayfs + `pivot_root`. Saves ~700 ms per launch and reduces per-VM disk to `O(actual writes)`.
- **Mechanics are settled by the literature.** Firecracker `is_read_only: true` is documented and tested for N-VM RO-base sharing; the kernel page-cache deduplicates (`firecracker-shared-rootfs.md`). overlayfs with ext4 lower + ext4 upper on different superblocks is supported; only `upperdir` and `workdir` must share a superblock (`overlayfs-kernel-semantics.md`). The `pivot_root(".",".")` sequence used by runc/crun/kata is directly liftable (`runc-crun-overlayfs-init.md`, `kata-containers.md` with verbatim source at `src/agent/rustjail/src/mount.rs:523`).
- **One new L1 epic** — proposed ID `m80-ovrl` ("overlay rootfs"). Eight leaves: DESIGN, m80-storage IMPL, m80-firecracker IMPL, m80-guestd IMPL (lifts kata's `pivot_rootfs`), m80-image-build kernel-config check, tests, bench, docs/migration. DESIGN gates all IMPL; m80-image-build kernel verification gates m80-guestd end-to-end.
- **Existing beads:** close three open premise-refuted leaves under `m80-urc.1` and the `m80-urc.1` parent itself; append "Update 2026-05-04" notes to the four already-closed beads with stale premise (`m80-urc.1.1`, `m80-urc.1.4`, `m80-6a0q.3`, `m80-6a0q.4`); append context paragraphs to adjacent beads (`m80-rrp.3`, `m80-bgas`). Spawn one follow-up leaf off `m80-6a0q.3` to update the workspace mount target from `/dev/vdb` to `/dev/vdc`.
- **Open questions for DESIGN to resolve before IMPL:** (a) does the firecracker-ci 5.10.245 kernel ship `CONFIG_OVERLAY_FS=y`; (b) PID-1 panic vs. retry on overlayfs mount failure; (c) where does the workspace mount live now that drive layout shifted (vdb → vdc); (d) is the loaded-cell stress-ng failure orthogonal to storage (working hypothesis: yes, but bench will confirm).

---

## 1. The new L1 epic + sub-leaves

### Epic: `m80-ovrl` — Shared RO base + per-VM overlay rootfs

```
id: m80-ovrl
title: Shared RO base + per-VM overlay rootfs (storage pivot)
status: open
priority: 1
labels: [storage, firecracker, performance, active-v0.1]
dependencies: []
```

**Description**

Replace the per-VM full file-copy of the base ext4 (`Rootfs::clone`, ~727 ms on a 256 MiB image) with a shared read-only host-side base file plus a per-VM sparse ext4 overlay assembled in-guest as overlayfs. The host file is presented to every VM as drive 1 with `is_read_only: true`; the per-VM sparse ext4 is drive 2; an optional workspace ext4 becomes drive 3. m80-guestd, running as PID 1 on the minimal-init image, mounts `/dev/vda` read-only as the lowerdir, `/dev/vdb` rw as the upperdir/workdir host, assembles overlayfs at a staging mount, then `pivot_root`s into it before mounting the workspace at `/workspace`.

**Why this exists.** Two wins: (a) ~700 ms of cold-launch latency from skipping the file-copy, plus the working-set pressure of 256 MiB pages no longer being newly populated for each VM. (b) per-VM disk usage drops from `O(base size)` to `O(actual writes)`, which at scale (thousands of concurrent VMs) is the difference between "cheap" and "needs a tier of fast block storage". Firecracker's own snapshot docs explicitly bless this pattern (`docs/snapshotting/snapshot-support.md:77`) and the test framework already shares squashfs-backed RO rootfs across N VMs (`tests/framework/microvm.py:1309-1313`).

**Why now.** The premise of `m80-urc.1` (per-VM rootfs cloning) is refuted by direct measurement plus the `firecracker-shared-rootfs.md` and `kata-containers.md` exploration. We are landing the minimal-init epic (`m80-6a0q`) at the same moment, and the new PID-1 path is the clean place to install overlayfs setup; doing it now avoids landing two PID-1 setup variants.

**Non-goals.**
- Multi-layer EROFS stacking (kata-style image-layer dedup). m80 has one base + one overlay; a single lowerdir is sufficient.
- Host-side device-mapper thin pools. Per-VM cost of a sparse `mkfs.ext4` is acceptable at our target VM creation rate; thin-pool complexity is unjustified for v0.1.
- tmpfs upper layer (firecracker-containerd's default). Bounded by guest RAM and incompatible with overlay-extraction. We use a real ext4 sparse file.
- Snapshot-resume integration. Complementary work; tracked in `m80-rrp.3`. Overlay-rootfs makes warm-start cheaper but is not blocked on snapshot.
- Live writeback of overlay contents during VM run. `Rootfs` does not extract; that is `Scratch`'s job and stays scoped to the workspace.
- Removing the ubuntu/systemd image. Out of scope; stays as the resilient fallback per `m80-6a0q`.

---

### Leaf 1: DESIGN — lock the API, drive layout, in-guest setup

```
id: m80-ovrl.1
title: DESIGN — lock storage API, drive layout, in-guest mount sequence
status: open
priority: 1
labels: [storage, design, active-v0.1]
dependencies: [m80-ovrl]
```

**Description.** Produce a written design that pins, in one place, the contract every other leaf in `m80-ovrl` consumes:

1. **m80-storage public surface (final form).** `Rootfs::prepare(base, overlay_dest, overlay_size_bytes, clone_mode) -> Result<Rootfs, StorageError>`, `Rootfs::new_at`, `Rootfs::base_path`, `Rootfs::overlay_path`. `Rootfs::clone` and `Rootfs::path` deleted. Behavioural notes: caller verifies base sha256, `prepare` allocates sparse + `mkfs.ext4 -F`.
2. **Drive layout, PUT order.** vda = base RO, vdb = overlay RW, vdc = workspace RW (when present). PUT order matches; `is_read_only: true` on vda; root is vda; ACPI DSDT order is the documented contract (`firecracker-shared-rootfs.md` §2).
3. **In-guest sequence (PID 1).** The 11-step mount/pivot pseudocode from `overlayfs-kernel-semantics.md` §"Minimum Mount Call Sequence" is normative: MS_REC|MS_PRIVATE on `/`, mount vda RO at `/lower`, vdb at `/upper`, mkdir `/upper/root` and `/upper/.work`, mount overlay at `/merged`, mount `/proc`/`/sys`/`/dev` *into* `/merged/...`, MS_SLAVE|MS_REC on `/`, bind `/merged` onto itself, `pivot_root(".", ".")` per kata, MNT_DETACH old root, then mount `/dev/vdc` at `/workspace` *inside the new root*.
4. **Failure policy.** Overlay mount or pivot failure as PID 1 panics (no kernel panic recovery; the run-dir is preserved for triage). No retry — failure here is structural and a retry hides it.
5. **Sparse overlay sizing.** Default 512 MiB sparse; configurable via `SandboxConfig::overlay_size_bytes`.

**Acceptance.**
- Doc landed at `docs/design/storage-overlay.md` with the API signatures, drive table, and pseudocode in code blocks. Cross-references to the four exploration docs.
- Both `crates/m80-storage/README.md` and `crates/m80-firecracker/README.md` already reflect this; sanity-check the two READMEs against the design doc and reconcile any drift in the same diff.
- No code in this leaf. This is a contract leaf; subsequent leaves consume it.

**Effort.** S (3–4 hours). Mostly captures decisions already made in the exploration docs.

---

### Leaf 2: IMPL m80-storage — `Rootfs::prepare` + sparse overlay

```
id: m80-ovrl.2
title: IMPL m80-storage — prepare(base, overlay_dest, size); delete clone()
status: open
priority: 1
labels: [storage, active-v0.1]
dependencies: [m80-ovrl.1]
```

**Description.** Replace `Rootfs::clone(base, dest)` with `Rootfs::prepare(base, overlay_dest, overlay_size_bytes, clone_mode)`. `prepare` allocates a sparse file at `overlay_dest` (use `File::create` + `set_len` — sparse on ext4/xfs/tmpfs by default), then shells out `mkfs.ext4 -F <overlay_dest>`. Returns `Rootfs { base, overlay }`. Add `base_path()` and `overlay_path()` accessors. Delete `Rootfs::clone`, `Rootfs::path()`, the `BaseSha256Mismatch` and `CopyRootfs` error variants, and any sha256-on-clone behavior — per the README, base verification is the caller's job (`m80-image-manifest::Manifest::verify`), not `Rootfs`'s.

**Acceptance.**
- `crates/m80-storage/src/lib.rs`: `Rootfs::prepare`, `new_at`, `base_path`, `overlay_path` present; `clone` and `path` absent. `mkfs.ext4 -F` shellout via existing `Command` pattern; bubble exit code as `StorageError::Mkfs`.
- `crates/m80-storage/tests/rootfs_prepare.rs`: at least four `#[test]` fns — sparse-overlay creation succeeds, base path preserved, overlay is a valid ext4 (run `file <overlay>` or read superblock magic), `new_at()` returns paths without I/O, missing-base path produces `StorageError::Io`.
- README "Public surface (full)" matches code 1:1; the "Migration note" section already there is accurate post-merge. Run `cargo test -p m80-storage` clean.

**Effort.** M (1 day). Touches a small surface but every consumer must rebuild.

---

### Leaf 3: IMPL m80-firecracker — three-drive PUT, RO base, RW overlay + workspace

```
id: m80-ovrl.3
title: IMPL m80-firecracker — three-drive PUT in PUT-order with is_read_only on base
status: open
priority: 1
labels: [firecracker, storage, active-v0.1]
dependencies: [m80-ovrl.1, m80-ovrl.2]
```

**Description.** Update the boot-phase block-device PUT sequence to (1) PUT `rootfs` with `is_read_only: true` and `is_root_device: true`, host_path = `Rootfs::base_path()`; (2) PUT `rootfs_overlay` with `is_read_only: false`, host_path = `Rootfs::overlay_path()`; (3) PUT `workspace` with `is_read_only: false`, host_path = `Scratch::path()`, only when `SandboxConfig::workspace_dir.is_some()`. Order is load-bearing (Firecracker assigns vda/vdb/vdc by PUT order — `firecracker-shared-rootfs.md` §2). The base file is bind-mounted into the jailer chroot exactly as today; nothing changes about the jailer surface beyond an extra bind-mount for the overlay file.

**Acceptance.**
- `crates/m80-firecracker/src/...` (boot-phase module — likely `src/boot.rs` or equivalent): three drive PUTs in the specified order; `is_read_only` field set explicitly per drive (no defaulting).
- Integration test (root-required, gated `#[ignore]`): launch a VM, attach to its admin socket, GET drives, assert vda is RO and vdb/vdc are RW.
- Update the README drive-layout table only if the existing one is wrong; spot-check it against the implementation. The README already shows the table — the diff should be code-only if the README is correct.

**Effort.** S (4 hours). Mechanical PUT-order refactor.

---

### Leaf 4: IMPL m80-guestd — overlayfs setup + `pivot_root` in PID-1

```
id: m80-ovrl.4
title: IMPL m80-guestd — overlayfs assembly + pivot_root before workspace mount
status: open
priority: 1
labels: [guestd, storage, active-v0.1]
dependencies: [m80-ovrl.1, m80-ovrl.5]
```

**Description.** Extend the PID-1 startup path in m80-guestd (the path established by `m80-6a0q.2`) to assemble the overlayfs and pivot into it before the workspace mount. The order matches `runc-crun-overlayfs-init.md` §4 and the kata `pivot_rootfs` body verbatim. The actual `pivot_root(".",".")` function is **lifted from kata-containers** (`src/agent/rustjail/src/mount.rs:523-559`); attribution comment and SPDX line as specified in `kata-containers.md` §5. Keep the `#[cfg(not(test))]` / `#[cfg(test)]` shim split — unit tests must not actually call the syscall.

Pseudocode (normative; lift the function bodies, not just the structure):

```rust
// Phase 1: mount layer disks.
fs::create_dir_all("/lower")?;
mount("/dev/vda", "/lower", "ext4", MS_RDONLY, None)?;
fs::create_dir_all("/upper")?;
mount("/dev/vdb", "/upper", "ext4", empty(), None)?;

// Phase 2: prepare overlay dirs (must be on /upper's superblock).
fs::create_dir_all("/upper/root")?;
fs::create_dir_all("/upper/.work")?;   // must be empty
fs::create_dir_all("/merged")?;

// Phase 3: overlayfs.
let opts = "lowerdir=/lower,upperdir=/upper/root,workdir=/upper/.work";
mount("overlay", "/merged", "overlay", empty(), Some(opts))?;

// Phase 4: /proc, /sys, /dev INTO /merged (before pivot).
//   exact flags per runc-crun-overlayfs-init.md §4 phase 4.

// Phase 5: propagation + bind-mount-self.
mount(None, "/", None, MS_SLAVE | MS_REC, None)?;
mount("/merged", "/merged", None, MS_BIND | MS_REC, None)?;

// Phase 6: pivot_rootfs("/merged")  -- lifted verbatim from kata.

// Phase 7: workspace inside the new root (only if /dev/vdc exists).
if Path::new("/dev/vdc").exists() {
    mount("/dev/vdc", "/workspace", "ext4", empty(), None)?;
}
```

**Acceptance.**
- `crates/m80-guestd/src/pid_one.rs` (or wherever PID-1 setup lives) contains the assembly + pivot, with the kata `pivot_rootfs` lift carrying SPDX-Apache-2.0 attribution.
- `Cargo.toml` for m80-guestd adds `scopeguard = "1"` (for `defer!`).
- Unit test: `tests/pid_one_pivot.rs::test_pivot_root_shimmed` — runs the function with the cfg(test) shim, asserts the FD-open and chdir path. Pattern matches kata's own test (`src/agent/rustjail/src/mount.rs:1269`).
- Integration test: a guest binary that boots in the m80-image-build minimal image, executes the full sequence, and writes a single byte to `/probe-after-pivot`. Host-side asserts the probe byte is on the overlay disk and not on the base disk after VM teardown.
- File-size: keep `pid_one.rs` under 500 lines; split into `pid_one/{mounts.rs,pivot.rs,workspace.rs}` if it crosses.

**Effort.** L (2–3 days). Lifting is fast; integration shake-out under real boot is the time sink.

---

### Leaf 5: IMPL m80-image-build — verify `CONFIG_OVERLAY_FS=y` in firecracker-ci kernel

```
id: m80-ovrl.5
title: IMPL m80-image-build — verify (or build) kernel with CONFIG_OVERLAY_FS=y
status: open
priority: 1
labels: [image-build, kernel, active-v0.1]
dependencies: [m80-ovrl.1]
```

**Description.** Verify the firecracker-ci 5.10.245 kernel currently consumed by m80-image-build has `CONFIG_OVERLAY_FS=y` (built-in, not module — modules are not loadable before init runs). If yes: add a preflight assertion in m80-image-build that fails the manifest-write step when the kernel lacks the symbol. If no: produce a custom kernel artifact under `crates/m80-image-build/kernels/` with the smallest config delta needed (`CONFIG_OVERLAY_FS=y` plus `CONFIG_OVERLAY_FS_XINO_AUTO=y` for free `st_ino` correctness — see `overlayfs-kernel-semantics.md` §5–6).

**Acceptance.**
- Either: a small Rust integration test that opens the kernel image, locates the embedded `IKCFG` blob (via `extract-ikconfig` logic if the kernel was built with `CONFIG_IKCONFIG_PROC=y`) and asserts `CONFIG_OVERLAY_FS=y` is present. **OR** a documented manual verification step in `docs/design/storage-overlay.md` plus a runtime assertion that overlayfs mount succeeds during guestd boot (covered by Leaf 4's integration test).
- If a custom kernel is needed: build script produces `kernels/vmlinux-overlay-<sha>.bin`, manifest schema gains `kernel.kind` discriminator parallel to image kind.
- Doc note: which kernel ships, where it came from, what config delta (if any).

**Effort.** S–M depending on outcome. S (2 hours) if the upstream config already has it; M (1 day) if a custom build is required.

---

### Leaf 6: TESTS — overlayfs assembly under unit + integration

```
id: m80-ovrl.6
title: TESTS — Rootfs::prepare unit, in-VM overlay+pivot integration, teardown ordering
status: open
priority: 2
labels: [tests, storage, active-v0.1]
dependencies: [m80-ovrl.2, m80-ovrl.4]
```

**Description.** Three test layers:

1. **Unit (m80-storage):** `tests/rootfs_prepare.rs` covers `Rootfs::prepare` happy path + four error scenarios (missing base, unwritable overlay path, mkfs failure, zero-size overlay). One scenario per `#[test]` fn (CLAUDE.md "no bundling").
2. **Unit (m80-guestd):** `tests/pid_one_pivot.rs` exercises the kata-shimmed `pivot_rootfs` (the syscall is stubbed under cfg(test)) and `tests/pid_one_overlay_paths.rs` exercises the path-assembly logic without doing the actual mount (factor pure helpers out of the mount sequence to make this testable).
3. **Integration (root + KVM, `#[ignore]`):** `tests/integration_overlay_lifecycle.rs` boots a VM with the minimal image, writes `/etc/m80-probe` (lands in overlay), reads `/etc/os-release` (served from base), exits, and verifies on the host: base ext4 is byte-identical pre/post, overlay contains exactly the new file, no symlink/special-file leakage.

**Acceptance.**
- All three test files present with multiple `#[test]` fns each; integration tests pass via `sudo -E cargo test -- --ignored` on the dev box.
- Teardown ordering test: in the integration suite, simulate a guestd panic mid-overlay-assembly and assert that the VM-launch admission permit is correctly dropped and the run-dir is preserved (consistent with `m80-firecracker`'s preserved-for-triage policy).

**Effort.** M (1 day). Most cost is in the integration harness wiring.

---

### Leaf 7: BENCH — re-run cold-launch.csv, pin storage_prep + ready_probe deltas

```
id: m80-ovrl.7
title: BENCH — cold-launch.csv re-run; pin storage_prep + ready_probe savings
status: open
priority: 2
labels: [bench, performance, active-v0.1]
dependencies: [m80-ovrl.4, m80-ovrl.5]
```

**Description.** Re-run the cold-launch microbenchmark suite (the one that today reports `storage_prep ≈ 727 ms`) on identical hardware to the last baseline, comparing pre-pivot to post-pivot for: storage_prep, kernel_boot, ready_probe (e2e p50/p95), 1-VM and 16-VM concurrent launch. The numerical result lives in this bead's notes after measurement; do not pre-write a target. Also run a "loaded cell" pass with stress-ng pinning host CPUs to baseline whether the storage pivot perturbs the known stress-ng-100% vsock failure (working hypothesis: orthogonal — failure is in vsock/timing under saturation, not storage; document either way).

**Acceptance.**
- `bench/cold-launch.csv` updated with both pre-pivot and post-pivot rows; commit message references this bead.
- A short note in `docs/design/storage-overlay.md` "Measured impact" section with the three deltas and a one-line interpretation (saturated win / mixed / regression).
- Stress-ng-loaded-cell run also recorded; if storage was *not* the cause of the failure, that data point informs the orthogonal vsock work.

**Effort.** S (3 hours). Run-and-record; the pipeline already exists.

---

### Leaf 8: DOCS + MIGRATION — sync exploration retractions, CHANGELOG, smolvm note

```
id: m80-ovrl.8
title: DOCS — exploration retractions, CHANGELOG entry, smolvm-exploration retract
status: open
priority: 2
labels: [docs, active-v0.1]
dependencies: [m80-ovrl.2, m80-ovrl.3, m80-ovrl.4]
```

**Description.** Land the documentation diffs:

1. `crates/m80-storage/README.md` — verify the "Migration note (v0.1 → v0.1.x)" section accurately describes the landed change; correct any drift. Confirm the "ext4-clone-and-scratch model is intentional" Non-goals line is gone (it was already removed in the draft).
2. `crates/m80-firecracker/README.md` — verify the drive-layout table matches what `m80-ovrl.3` shipped.
3. `CHANGELOG.md` — entry under v0.1.x: "Storage: replaced per-VM rootfs clone with shared RO base + per-VM sparse overlay + in-guest overlayfs. ~700 ms cold-launch reduction. `Rootfs::clone` removed; `Rootfs::prepare` added."
4. `docs/exploration/smolvm-*.md` (if any claims that the file-copy is intentional remain) — append a "Retracted 2026-05-04" footer naming this bead.
5. `docs/exploration/storage-pivot.md` (if it exists) — review and reconcile against the landed code; this is exploration-era prose, not authoritative.

**Acceptance.**
- All four/five doc files updated in one diff. No drift between READMEs and the code as it lands. CHANGELOG entry under the correct version section.

**Effort.** S (1–2 hours).

---

### Dependency graph

```
m80-ovrl
  └─ m80-ovrl.1 DESIGN
       ├─ m80-ovrl.2 storage IMPL
       │    └─ m80-ovrl.3 firecracker IMPL
       │         └─ m80-ovrl.6 tests
       ├─ m80-ovrl.5 image-build kernel
       │    └─ m80-ovrl.4 guestd IMPL ──┐
       │                                ├─→ m80-ovrl.6 tests
       │                                └─→ m80-ovrl.7 bench
       │                                              └─→ m80-ovrl.8 docs
       └─ m80-ovrl.4 also depends on m80-ovrl.1
```

DESIGN gates everything. Storage IMPL and firecracker IMPL can land before guestd IMPL (the guest-side will not work end-to-end without all three, but they don't compile-block each other). Tests + bench + docs land last.

---

## 2. Existing-bead-state changes

### `m80-urc.1` — Per-VM rootfs cloning (open) → close as premise-refuted

Action: `br update m80-urc.1 --notes "premise-refuted: shared RO base + per-VM overlay supersedes cloning. See m80-ovrl."` then `br close m80-urc.1 --reason "premise-refuted"`.

Replacement description text (what the bead should be set to before close):

```
[CLOSED 2026-05-04 — PREMISE REFUTED]

Original premise: each VM gets a private byte-for-byte copy of the base ext4
under its run-dir. Refuted by direct measurement (727 ms storage_prep per
launch, ~256 MiB working-set churn) and by the firecracker-shared-rootfs.md
exploration showing N-VM read-only sharing of the same host file is safe and
explicitly supported by Firecracker.

Superseded by m80-ovrl: shared RO base file (single inode, page-cache
deduplicated) + per-VM sparse ext4 overlay + in-guest overlayfs. The four
sub-leaves (cloning, parent-dir creation, base-identity pre-clone verify,
CopyRootfs error) are obsolete.
```

### `m80-urc.1.2` — Create runtime rootfs parent directories before copy (open) → close as premise-refuted

Action: close. Replacement text:

```
[CLOSED 2026-05-04 — PREMISE REFUTED]

There is no per-VM rootfs copy in the m80-ovrl design; the overlay-dest path
is created by m80-firecracker's run-dir setup (existing behavior, separate
bead). No standalone parent-dir-creation step inside Rootfs::prepare —
Rootfs::prepare returns Io error if overlay_dest's parent does not exist (per
CLAUDE.md "no silent recovery").
```

### `m80-urc.1.3` — Verify managed rootfs boot identity before clone (open) → close as premise-refuted

Action: close. Replacement text:

```
[CLOSED 2026-05-04 — PREMISE REFUTED]

The "before clone" coupling is gone — there is no clone. Base sha256
verification remains a thing, but it is the caller's responsibility before
calling Rootfs::prepare, exactly as documented in
crates/m80-storage/README.md "Public API" (m80_image_manifest::Manifest::verify).
This bead's behavior is now subsumed by image-manifest verification at the
firecracker boot phase, which is its own pre-existing leaf.
```

### `m80-urc` — Storage & Filesystem (epic, open) — append note

Action: `br update m80-urc --notes "post-2026-05-04 storage pivot: see m80-ovrl"`. Append to description:

```
**Update 2026-05-04 (post-storage-pivot):** Sub-epic m80-urc.1 (per-VM rootfs
cloning) is closed premise-refuted. The cloning behavior is replaced by
shared-RO-base + per-VM-sparse-overlay; that work lives under m80-ovrl, not
under m80-urc. m80-urc retains responsibility for scratch (workspace) image
creation, hydration, and post-stop change extraction, which are unaffected
by the rootfs pivot.
```

### `m80-urc.1.1` — Clone managed rootfs into per-VM runtime image (closed) — append "Update" note

**Do not reopen.** It was an honest closure of the predecessor behavior at the time. Append to description:

```
**Update 2026-05-04 (post-storage-pivot):** This behavior was correctly
captured at v0.1 close. As of m80-ovrl, the cloning architecture is
replaced by shared RO base + per-VM sparse overlay + in-guest overlayfs.
The captured doc and test remain accurate as predecessor-era behavior
description; they should be left in place as historical reference. New
behavior is captured under m80-ovrl.2.
```

### `m80-urc.1.4` — Surface CopyRootfs error with both paths (closed) — append "Update" note

Append to description:

```
**Update 2026-05-04 (post-storage-pivot):** CopyRootfs error is being
removed from StorageError as part of m80-ovrl.2 — there is no copy
operation to fail. The original behavior was correct for the v0.1 cloning
path; it does not apply post-pivot. The captured test
(per_vm_rootfs_clone.rs::copy_error_carries_paths) is being deleted in
the same diff as m80-ovrl.2.
```

### `m80-6a0q.3` — Workspace mount via mount(2) when PID-1 (closed) — append "Update" note + spawn follow-up

Append to description:

```
**Update 2026-05-04 (post-storage-pivot):** The constants in this bead
referenced /dev/vdb as the workspace device. After m80-ovrl, the drive
layout is vda=base RO, vdb=overlay RW, vdc=workspace RW. The workspace
mount must move from /dev/vdb to /dev/vdc, and it must occur **inside the
pivoted root** (after pivot_root, not before). Follow-up leaf m80-ovrl.4a
captures this update.
```

Spawn a small follow-up leaf:

```
id: m80-ovrl.4a
title: Update workspace mount target from /dev/vdb to /dev/vdc inside pivoted root
status: open
priority: 1
labels: [guestd, storage, active-v0.1]
dependencies: [m80-ovrl.4]

Description: Update the constants and mount call in m80-guestd's PID-1
workspace step (originally captured by m80-6a0q.3) to use /dev/vdc rather
than /dev/vdb, and to be the *last* mount step (after pivot_root). The
"no scratch drive" silent-skip behavior remains: if /dev/vdc does not
exist, skip without error. Update or replace the captured doc at
docs/behaviors/storage/workspace-mount.md and the test at
crates/m80-guestd/tests/storage/workspace_mount.rs accordingly.

Acceptance: existing workspace_mount test passes against /dev/vdc; the
"no drive" skip path is exercised; doc reflects post-pivot mount target.

Effort: S (1 hour). Mechanical constant change + one test path update.
```

### `m80-6a0q.4` — Minimal initramfs/rootfs in m80-image-build (closed) — append "Update" note

Append to description:

```
**Update 2026-05-04 (post-storage-pivot):** The minimal-init image
produced by this bead becomes the default carrier of the overlayfs+pivot
setup added by m80-ovrl.4. No change to the image-build artifacts
themselves is required by the storage pivot — the rootfs that
m80-image-build produces still becomes /dev/vda in the guest, which is
now mounted read-only as the lowerdir rather than directly as /. Kernel
config gains an explicit verification of CONFIG_OVERLAY_FS=y under
m80-ovrl.5; this bead's captured doc/test stay correct.
```

### `m80-rrp.3` — Snapshot-restore launch path (open) — append context paragraph

Append to description:

```
**Update 2026-05-04 (post-storage-pivot):** Snapshot-restore is
complementary to the m80-ovrl overlay rootfs work — they compose, neither
blocks the other. Specifically: Firecracker snapshot-resume does not care
how the rootfs is structured; it captures the post-boot in-memory state.
A VM restored from snapshot inherits the overlayfs mount tree from the
snapshot moment, which is fine. One subtle interaction to validate when
this bead lands: the overlay disk (vdb) must be present and at the same
filesystem-level state on resume as it was at snapshot time. For the
"diff snapshot off a clean post-boot snapshot" use case, vdb should be
freshly-formatted-empty at both capture and restore. This is a constraint
to verify in m80-rrp.3's tests, not a blocker.
```

### `m80-bgas` — Quick wins in phase_12b_ready_probe (closed) — append "Update" note

Append to description:

```
**Update 2026-05-04 (post-storage-pivot):** The 10ms/60s tuning landed
under the assumption of the v0.1 clone-and-boot launch path. After
m80-ovrl, storage_prep drops by ~700 ms, which means the host-side
launch-to-ready_probe budget shifts: ready_probe will start firing
earlier in wall-clock from launch(). The 10 ms poll interval and 60 s
timeout remain correct; no retune required. The bench leaf m80-ovrl.7
should record the new e2e ready_probe distribution as a baseline for any
future retune.
```

---

## 3. Risks / open questions to resolve in DESIGN

These are the questions the literature does not fully answer; DESIGN (`m80-ovrl.1`) must close each before downstream IMPL leaves move:

**1. CONFIG_OVERLAY_FS in the firecracker-ci kernel.** `overlayfs-kernel-semantics.md` §6 states that the firecracker-ci 5.10.245 kernel "historically includes" `CONFIG_OVERLAY_FS=y` but does not verify it. This is not optional — without `=y` (built-in), the overlayfs mount in PID-1 will fail because module loading is not yet up. Resolution path: extract `IKCFG` from the kernel image (if `CONFIG_IKCONFIG_PROC=y` is set, it is) and grep. Failing that, do a minimal one-shot boot test that runs `cat /proc/filesystems | grep overlay` from inside a Ubuntu-init guest. If the symbol is missing, m80-ovrl.5 escalates from "verify" to "build". Cost difference: small (custom kernel build is well-trodden).

**2. PID-1 panic vs. retry on overlayfs mount failure.** A failing overlayfs mount as PID 1 has no clean recovery — there is no `/sbin/init` to fall back to in the minimal image. `overlayfs-kernel-semantics.md` §8 says cleanly unmount upper and lower with `MNT_DETACH` and panic. The kernel's response to PID 1 exit is a kernel panic, which is what we want for a failed-fast signal: m80 host sees the VM die in boot phase, preserves the run-dir, returns a typed `FcError::GuestBootFailed { phase: "overlay-mount", errno }`. **No retry.** Per CLAUDE.md "no silent recovery" — failing here is structural; retry hides it.

**3. Workspace mount target after pivot.** The /workspace dir must exist in the pivoted root for the bind mount target to be present. `/workspace` lives on the lower (base) filesystem — it was created at image-build time and is part of the read-only base. Post-pivot it surfaces in the merged view via the lower layer. **It survives the pivot trivially** because the lowerdir contributes it. No special handling required — but DESIGN should pin this with one-line note in `docs/design/storage-overlay.md`. Captured under `m80-ovrl.4a`.

**4. Pre-population of `/upper/root` and `/upper/.work`.** overlayfs requires the workdir empty at mount time. We cannot pre-populate it at image-build (that is the base, which is the lower layer, not the overlay). The overlay disk is freshly formatted by `Rootfs::prepare` (sparse + `mkfs.ext4 -F`); a freshly-formatted ext4 is empty. m80-guestd creates the two subdirs (`mkdir /upper/root /upper/.work`) at PID-1 time, after mounting `/dev/vdb`. This is the only viable spot. DESIGN should record the chronology explicitly: format at host launch time → mount in guest → mkdir in guest → mount overlay in guest.

**5. Loaded-cell stress-ng failure interaction.** Working hypothesis: orthogonal. The known stress-ng-100% failure is in the vsock readiness handshake under CPU saturation, not in storage I/O. Storage_prep happens *before* the kernel boots; CPU saturation does not affect a `mkfs.ext4` on a sparse file (~10 ms). The hypothesis is testable in `m80-ovrl.7` by running the stress-ng cell with overlay-rootfs enabled — if failure rate is unchanged, hypothesis confirmed; if it changes, we have a new data point. **Document the hypothesis in DESIGN; let the bench leaf falsify or confirm.**

**6. Sparse file semantics across host filesystems.** `File::create` + `set_len(N)` produces a sparse file on ext4, xfs, btrfs, and tmpfs. On older filesystems (or some FUSE mounts) it may pre-allocate. DESIGN should pin the assumption: m80's run-dir is on a Linux native filesystem; if a deployer puts it on FUSE or a non-sparse-supporting fs, `Rootfs::prepare` may stall at allocation. Add a one-line preflight check in `m80-preflight` that the run-dir filesystem supports sparse files (e.g. `statfs` and reject known-bad fstypes). Low priority; defer to its own leaf if it becomes a real problem.

**7. Teardown ordering.** Kata's `multi_layer_erofs.rs` returns `temp_mount_points` in the order overlay → upper → lower for teardown. m80's case is simpler (one upper, one lower), but the ordering matters: overlay first, then upper, then lower. DESIGN should pin this. In our case teardown happens implicitly when the VM exits — the kernel tears down all its mounts. The host side has nothing to unmount because the overlay assembly happened in-guest. So this question is mostly moot for v0.1; flag it as a concern only if we add an "in-host overlay rebuild for forensics" feature later.

**8. License attribution.** Kata is Apache-2.0 and the verbatim lift of `pivot_rootfs` requires the SPDX header in `kata-containers.md` §5. DESIGN should pin the attribution comment block; m80-ovrl.4 reproduces it. No legal risk; mechanical bookkeeping.

---

## Closing notes

This plan is opinionated about ordering: DESIGN (`m80-ovrl.1`) before all IMPL; m80-image-build kernel verification (`m80-ovrl.5`) before m80-guestd IMPL (`m80-ovrl.4`) because the guest path is structurally unworkable without the kernel symbol. m80-storage and m80-firecracker IMPL can proceed in parallel after DESIGN. Tests, bench, and docs cluster at the end.

The most directly liftable piece of code is kata's `pivot_rootfs` (`/tank/projects/kata-containers/src/agent/rustjail/src/mount.rs:523-559`), Apache-2.0; everything else is fresh code informed by the four exploration documents. The single highest-risk item is kernel `CONFIG_OVERLAY_FS=y` — verify before committing to the IMPL leaves.

No new infrastructure crates are proposed (per CLAUDE.md "no junk drawers"). No abstractions across the storage boundary (per "no premature abstraction"). One concrete impl, three deletions, eight new leaves, one follow-up under an existing closed bead. Total effort: roughly 5–7 person-days end to end.
