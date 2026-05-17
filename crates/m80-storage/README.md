# `m80-storage`

Per-VM **rootfs view** (shared read-only base + sparse writable overlay),
scratch ext4 image creation + hydration, and **opt-in** post-stop change
extraction with admissibility scan and atomic swap.

## Reason for being

Storage in m80 has three independent concerns that historically tangle:
keeping the base rootfs immutable across VMs, giving each VM a private
writable view of it (overlay), giving each VM a separate writable
**workspace** device (scratch image), and — optionally — extracting
modified files back to the host after the VM exits cleanly (change
extraction, the "writeback" model in predecessor).

Pulling them into one crate forces consistent guarantees across all
three: the same admissibility rules, the same atomic-swap semantics, the
same error vocabulary. It also lets us isolate the e2fsck/loop-mount
shell-out surface, which is the part most likely to misbehave under
filesystem stress.

The reframe relative to predecessor: change extraction is generic and
caller-controlled. There is no `EffectClass::Mutating` gate; the consumer
asks for extraction or it doesn't.

## Black-box contract

### Rootfs view: shared base + per-VM overlay

Each VM gets a logical `Rootfs` composed of two host files:

1. The **base ext4** (the artifact `m80-image-build` produces). Used
   read-only and shared across every VM that references it. There is
   **no per-VM copy of the base**.
2. A **per-VM overlay ext4** — cloned at launch from a run-root-local
   empty ext4 template, attached to the VM as a writable second drive.
   In-VM, `m80-guestd` mounts the base at `/lower`, the overlay at
   `/upper`, and overlayfs at `/`. Writes from the guest land in the
   overlay; the base is never touched.

The host page cache deduplicates: 100 VMs from the same image read the
base from one set of pages.

### Public API

- `Rootfs::prepare(base: &Path, overlay_dest: &Path, overlay_size_bytes: u64, clone_mode: OverlayTemplateCloneMode) -> Result<Rootfs, StorageError>`
  Ensures a run-root-local empty overlay template exists, then clones it
  to `overlay_dest` through the caller-selected clone mode. `ByteCopy`
  runs `cp --reflink=never --sparse=auto`; `Reflink` runs
  `cp --reflink=always --sparse=auto`; `Auto` probes the run-root
  filesystem once and selects one of those two concrete modes before
  cloning. Probe failures and concrete clone failures are hard errors;
  `Rootfs::prepare` does not retry as another mode. The returned `Rootfs`
  has `base_path()` set to the
  caller-supplied shared base and `overlay_path()` set to the new
  per-VM overlay. **Caller is responsible for sha256 verification of
  `base` via `m80_image_manifest::Manifest::verify()` before calling
  `prepare`; this function does not re-verify.** The template is
  formatted once with `mkfs.ext4 -F`, trimmed with `fallocate -d`, and
  guarded by metadata recording schema version, ext4 identity, size,
  mkfs command, and postprocess command; a stale or wrong-size template
  is a hard error.
- `Rootfs::new_at(base: &Path, overlay: &Path) -> Rootfs` — wraps a pair
  of existing paths without allocating; for tests and recovery scenarios.
- `Rootfs::base_path(&self) -> &Path` — the shared, read-only base ext4.
- `Rootfs::overlay_path(&self) -> &Path` — the per-VM writable overlay.
- `OverlayTemplateCloneMode` — `ByteCopy`, `Reflink`, or `Auto`.

### Scratch (workspace) — unchanged

- `Scratch::create(workspace: &Path, image: &Path, size: u64) ->
  Result<Scratch, StorageError>` formats a scratch ext4 image via
  `mkfs.ext4`, loop-mounts it, copies the host workspace tree, and
  unmounts. The host source is opaque to the guest — only the mounted
  device shows up in-VM.
- `Scratch::extract(image: &Path, into: &Path, max_extract_bytes: Option<u64>) -> Result<ChangeSet, StorageError>`
  is the post-stop extraction:
  0. Fail with `SwapFailed` if `into` already exists.
  1. Run `e2fsck -p -f` on the image to repair the journal.
  2. Loop-mount the image read-only.
  3. Walk and apply the admissibility scan: regular files and
     directories pass; symlinks → `RejectionReason::Symlink`; devices /
     fifos / sockets → `RejectionReason::SpecialFile`.
     Extracted file and directory permissions preserve only host-visible
     `0o777` bits; setuid, setgid, and sticky bits are stripped before
     publishing the staged tree.
     When `max_extract_bytes` is set, the walk fails with
     `StorageError::ExtractSizeExceeded` before copying a regular file that
     would push total extracted bytes above the cap.
  4. Stage the surviving set in a sibling temp directory under the
     destination parent, named `.<workspace>.m80-writeback-stage-<pid>-<n>`.
  5. Atomic `fs::rename` into `into`; rename failure is `SwapFailed`.
  6. Unmount.
  Only triggered when the caller asks.
- `ChangeSet` reports the staged file count, total bytes, and any
  rejected paths. The caller decides what to do with rejection reports.

The workspace scratch is **separate** from the rootfs overlay. The
rootfs overlay accumulates everything the guest writes anywhere under
`/`; the workspace scratch isolates the user's intended-output
directory so `Scratch::extract` doesn't have to discriminate user
output from system noise (logs, /tmp, /var/cache, etc.). Extraction
operates on the workspace, not the rootfs overlay.

### Operational notes

- All shell-outs (`mkfs.ext4`, `cp`, `fallocate`, `e2fsck`, `mount`, `umount`) bubble up
  exit codes as typed errors.
- Reflink use is a caller-selected optimization, not a requirement. The
  byte-copy overlay-template clone path remains permanent because ext4
  production hosts are valid. See
  [`docs/behaviors/storage/reflink-rootfs.md`](../../docs/behaviors/storage/reflink-rootfs.md)
  for the explicit policy contract.
- Concurrency: `Rootfs` and `Scratch` are NOT thread-safe. The
  orchestrator serializes per-VM storage operations.

## v0.1 departure: loop-mount instead of debugfs

predecessor uses `debugfs rdump` to extract files from the scratch image
without a remount. v0.1 uses `mount -o loop,ro` instead. Observable
behaviour is identical for the scratch image sizes m80 targets (64 MiB
– 512 MiB). The `debugfs` path would be more efficient for very large
images but adds non-trivial output-parsing surface.

## Public surface

- `Rootfs::prepare(base, overlay_dest, overlay_size_bytes, clone_mode)`,
  `Rootfs::new_at(base, overlay)`,
  `Rootfs::base_path()`, `Rootfs::overlay_path()`.
- `OverlayTemplateCloneMode`: `ByteCopy`, `Reflink`, `Auto`.
- `Scratch::create(workspace, image, size)`,
  `Scratch::extract(image, into, max_extract_bytes)`, `Scratch::path()`.
- `ChangeSet { staged: Vec<PathBuf>, rejected: Vec<Rejection>, total_bytes: u64 }`.
- `Rejection { path: PathBuf, reason: RejectionReason }`.
- `RejectionReason`: `Symlink`, `SpecialFile`.
- `StorageError`: `OverlayCreateFailed`,
  `OverlayTemplateCreateFailed`, `OverlayTemplateMismatch`,
  `OverlayTemplateCloneFailed`,
  `OverlayTemplateCloneModeProbeFailed`,
  `SubprocessFailed { program: &'static str, path: PathBuf, status: String, stderr: String }`,
  `AdmissibilityRefused { path: PathBuf }`,
  `ExtractSizeExceeded { path: PathBuf, max_bytes: u64, actual_bytes: u64 }`,
  `SwapFailed`, `Io { path: PathBuf, source: io::Error }`.

Removed variants:
- `BaseSha256Mismatch` — verification is the caller's responsibility
  (`m80_image_manifest::Manifest::verify`); this crate never produces it.
- `DebugfsFailed` — debugfs is not used in v0.1 (loop-mount instead).
- `Privilege` — `m80-privileged` was deleted; the m80 process holds
  `CAP_SYS_ADMIN` at startup (verified by `m80-preflight`).

## Non-goals

- **No live writeback.** Change extraction operates on a stopped VM's
  scratch image. There is no in-flight sync.
- **No alternative virtual-storage models.** virtiofs, NFS, 9p — out
  of scope. Read-only-base + virtio-blk overlay covers what we need on
  Firecracker without taking on a new device-model surface.
- **No "did the build succeed" inference.** `Scratch::extract` reports
  what changed; it does not interpret meaning.
- **No EffectClass.** Extraction happens iff the caller asks.
- **No sha256 verification inside `Rootfs::prepare`.** The caller
  verifies before calling.
- **No public scratch-sizing policy.** Callers pass the scratch image size
  they want to `Scratch::create`; m80's current internal recommendation is
  deliberately not part of the library contract.

## Dependencies

Runtime: `thiserror`, `tracing`, `serde`, `tempfile`, `nix`.
Dev: `sha2`, `hex`, `serde_json`, `nix` (root-check in integration tests).

Shell-outs require `CAP_SYS_ADMIN` (for `mount`/`umount`) and
`mkfs.ext4` / `cp` / `fallocate` / `e2fsck` on `PATH`; `m80-preflight`
verifies at startup.

## Tests

Non-root (always run):
- `tests/rootfs_prepare.rs` — sparse-overlay creation, base path
  preserved, overlay template metadata/reuse, stale-template hard error,
  `new_at()`, missing-parent error.
- `tests/scratch_admissibility.rs` — admissibility logic via inline
  classify function.
- `tests/changeset_serde.rs` — `ChangeSet`/`Rejection` JSON round-trips.
- `tests/error_round_trip.rs` — each `StorageError` variant displays
  sensibly.

Root/loop-mount (`#[ignore]`, run with `sudo cargo test -- --ignored`):
- `tests/scratch_create_real.rs` — hydration and symlink rejection.
- `tests/scratch_extract_real.rs` — full create→extract round trip;
  SwapFailed on existing `into`.
- `tests/scratch_image.rs` — behavior-capture fixtures for hydration.
- `tests/change_extraction.rs` — behavior-capture fixtures for opt-in
  extraction and rollback on destination failure.

Real-KVM smoke:
- `M80_VERIFY_REFLINK_DIVERGENCE=1 ./scripts/smoke.sh launch-only` —
  boots a VM, writes into the guest rootfs, and verifies the host overlay
  diverges while the template allocation stays unchanged.

## Migration note (v0.1 → v0.1.x; landing in same release line)

The previous API exposed `Rootfs::clone(base, dest) -> Rootfs` which
performed a **byte-for-byte copy** of the base ext4 to a per-VM
location, sized to match the base. That model is removed:

- The full-copy path was ~700 ms per launch on a 256 MiB rootfs (file
  copy + page cache miss). The shared-base + sparse-overlay model is
  sub-millisecond and reduces per-VM disk usage from `O(base size)` to
  `O(actual writes)`.
- `Rootfs::clone` is gone. Replace with `Rootfs::prepare(base,
  overlay_dest, overlay_size, clone_mode)`. `m80-firecracker` is the only caller in
  this workspace; the migration is mechanical. `Rootfs::prepare` now
  reuses a run-root-local empty ext4 template so the per-launch path does
  not run `mkfs.ext4`.
- `Rootfs::path()` is gone. The single-path model doesn't survive the
  split. Use `base_path()` or `overlay_path()` per intent.
- The "ext4-clone-and-scratch model is intentional" Non-goals line is
  retracted. The clone model was a v0.1 expedient; we measured and the
  expedient cost too much.
