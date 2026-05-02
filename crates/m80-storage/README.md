# `m80-storage`

Per-VM rootfs cloning, scratch ext4 image creation + hydration, and
**opt-in** post-stop change extraction with admissibility scan and
atomic swap.

## Reason for being

Storage in m80 has three independent concerns that historically tangle:
keeping the base rootfs immutable across VMs (cloning), giving each VM
a writable workspace device (scratch image), and — optionally —
extracting modified files back to the host after the VM exits cleanly
(change extraction, the "writeback" model in predecessor).

Pulling them into one crate forces consistent guarantees across all
three: the same admissibility rules, the same atomic-swap semantics, the
same error vocabulary. It also lets us isolate the e2fsck/loop-mount
shell-out surface, which is the part most likely to misbehave under
filesystem stress.

The reframe relative to predecessor: change extraction is generic and
caller-controlled. There is no `EffectClass::Mutating` gate; the consumer
asks for extraction or it doesn't.

## Black-box contract

- `Rootfs::clone(base: &Path, dest: &Path) -> Result<Rootfs, StorageError>`
  produces a per-VM byte-for-byte copy of an immutable base ext4. **Caller
  is responsible for sha256 verification via
  `m80_image_manifest::Manifest::verify()` before calling `clone`;
  this function does not re-verify.**
- `Rootfs::new_at(dest: &Path) -> Rootfs` wraps an existing path without
  copying — for tests and recovery scenarios.
- `Scratch::create(workspace: &Path, image: &Path, size: u64) ->
  Result<Scratch, StorageError>` formats a scratch ext4 image via
  `mkfs.ext4`, loop-mounts it, copies the host workspace tree, and
  unmounts. The host source is opaque to the guest — only the mounted
  device shows up in-VM.
- `Scratch::extract(image: &Path, into: &Path) -> Result<ChangeSet, StorageError>`
  is the post-stop extraction:
  1. Run `e2fsck -p -f` on the image to repair the journal.
  2. Loop-mount the image read-only.
  3. Walk and apply the admissibility scan: regular files and directories
     pass; symlinks → `RejectionReason::Symlink`; devices/fifos/sockets →
     `RejectionReason::SpecialFile`.
  4. Stage the surviving set in a temp directory.
  5. Atomic `fs::rename` into `into`. Fails with `SwapFailed` if `into`
     already exists.
  6. Unmount.
  This call is **only made if the caller asks**. There's no implicit
  trigger.
- `ChangeSet` reports the staged file count, total bytes, and any
  rejected paths (with reasons). The caller decides what to do with
  rejection reports.
- All shell-outs (`mkfs.ext4`, `e2fsck`, `mount`, `umount`) bubble up
  exit codes as typed errors.
- Concurrency: `Rootfs` and `Scratch` are NOT thread-safe. The
  orchestrator serializes per-VM storage operations.

## v0.1 departure: loop-mount instead of debugfs

predecessor uses `debugfs rdump` to extract files from the scratch image
without a remount. v0.1 uses `mount -o loop,ro` instead. Observable
behaviour is identical for the scratch image sizes m80 targets (64 MiB –
512 MiB). The `debugfs` path would be more efficient for very large images
but adds non-trivial output-parsing surface. The v0.1 implementation
uses loop-mount; a future revision can add debugfs for large-image
optimisation.

## Public surface

- `Rootfs::clone(base, dest)`, `Rootfs::new_at(dest)`, `Rootfs::path()`.
- `Scratch::create(workspace, image, size)`, `Scratch::extract(image, into)`,
  `Scratch::path()`.
- `ChangeSet { staged: Vec<PathBuf>, rejected: Vec<Rejection>, total_bytes: u64 }`.
- `Rejection { path: PathBuf, reason: RejectionReason }`.
- `RejectionReason`: `Symlink`, `SpecialFile`, `OutsideWorkspace`, `Other(String)`.
- `StorageError`: `Mkfs(io::Error)`, `E2fsckFailed { exit, stderr }`,
  `AdmissibilityRefused`, `SwapFailed`,
  `Io { path: PathBuf, source: io::Error }`.

Removed variants:
- `BaseSha256Mismatch` — verification is the caller's responsibility
  (`m80_image_manifest::Manifest::verify`); this crate never produces it.
- `DebugfsFailed` — debugfs is not used in v0.1 (loop-mount instead).
- `Privilege` — `m80-privileged` was deleted; the m80 process holds
  `CAP_SYS_ADMIN` at startup (verified by `m80-preflight`).

## Non-goals

- **No live writeback.** Change extraction operates on a stopped VM's
  scratch image. There is no in-flight sync.
- **No alternative storage models.** virtiofs, NFS, 9p — out of scope.
  The opinionated ext4-clone-and-scratch model is intentional.
- **No "did the build succeed" inference.** `Scratch::extract` reports
  what changed; it does not interpret meaning.
- **No EffectClass.** Extraction happens iff the caller asks.
- **No sha256 verification inside `Rootfs::clone`.** The caller verifies
  before cloning.

## Dependencies

Runtime: `thiserror`, `tracing`, `serde`, `tempfile`.  
Dev: `sha2`, `hex`, `serde_json`, `nix` (root-check in integration tests).

Shell-outs require `CAP_SYS_ADMIN` (for `mount`/`umount`) and
`mkfs.ext4`/`e2fsck` on `PATH`; `m80-preflight` verifies at startup.

## Tests

Non-root (always run):
- `tests/rootfs_clone.rs` — clone byte-identity, path(), new_at(), missing-base error.
- `tests/scratch_admissibility.rs` — admissibility logic via inline classify function.
- `tests/changeset_serde.rs` — `ChangeSet`/`Rejection` JSON round-trips.
- `tests/error_round_trip.rs` — each `StorageError` variant displays sensibly.

Root/loop-mount (`#[ignore]`, run with `sudo cargo test -- --ignored`):
- `tests/scratch_create_real.rs` — hydration and symlink rejection.
- `tests/scratch_extract_real.rs` — full create→extract round trip; SwapFailed on existing `into`.
