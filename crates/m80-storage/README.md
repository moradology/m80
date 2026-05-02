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
same error vocabulary. It also lets us isolate the e2fsck/debugfs/blkid
shell-out surface, which is the part most likely to misbehave under
filesystem stress.

The reframe relative to predecessor: change extraction is generic and
caller-controlled. There is no `EffectClass::Mutating` gate; the consumer
asks for extraction or it doesn't.

## Black-box contract

- `Rootfs::clone(base: &Path, dest: &Path) -> Result<Rootfs, StorageError>`
  produces a per-VM clone of an immutable base ext4. The base sha256 is
  verified against the manifest before cloning; mismatch is fatal.
- `Scratch::create(workspace: &Path, image: &Path, size: u64) ->
  Result<Scratch, StorageError>` formats a scratch ext4 image and
  hydrates it from the host workspace tree. The host source is opaque to
  the guest — only the mounted device shows up in-VM.
- `Scratch::extract(image: &Path, into: &Path) -> Result<ChangeSet, StorageError>`
  is the post-stop extraction:
  1. Run `e2fsck -p` on the image to repair the journal.
  2. Run `debugfs` to enumerate modified files (no remount).
  3. Apply the admissibility scan: regular files and directories only.
     Symlinks, devices, fifos, and sockets are rejected.
  4. Stage the surviving set in a temp directory.
  5. Atomic rename into `into`. Failure at any step rolls back fully —
     `into` is unchanged.
  This call is **only made if the caller asks**. There's no implicit
  trigger.
- `ChangeSet` reports the staged file count, total bytes, and any
  rejected paths (with reasons). The caller decides what to do with
  rejection reports.
- All shell-outs (`mkfs.ext4`, `e2fsck`, `debugfs`) bubble up exit codes
  as typed errors. m80 does not parse `debugfs` output; it uses the
  `dump` machine-readable mode.
- Concurrency: `Rootfs` and `Scratch` are NOT thread-safe. The
  orchestrator serializes per-VM storage operations. Cross-VM
  parallelism is fine because each VM has its own clone + scratch.

## Public surface

- `Rootfs::clone(base, dest)`, `Rootfs::path()`.
- `Scratch::create(workspace, image, size)`, `Scratch::extract(image, into)`,
  `Scratch::path()`.
- `ChangeSet { staged: Vec<PathBuf>, rejected: Vec<Rejection>, total_bytes: u64 }`.
- `Rejection { path: PathBuf, reason: RejectionReason }`.
- `RejectionReason`: `Symlink`, `SpecialFile`, `OutsideWorkspace`, `Other(String)`.
- `StorageError`: `BaseSha256Mismatch`, `Mkfs(io::Error)`,
  `E2fsckFailed { exit, stderr }`, `DebugfsFailed { exit, stderr }`,
  `AdmissibilityRefused`, `SwapFailed`, `Privilege(m80_privileged::PrivilegeError)`,
  `Io(io::Error)`.

## Non-goals

- **No live writeback.** Change extraction operates on a stopped VM's
  scratch image. There is no in-flight sync.
- **No alternative storage models.** virtiofs, NFS, 9p — out of scope.
  The opinionated ext4-clone-and-scratch model is intentional.
- **No "did the build succeed" inference.** `Scratch::extract` reports
  what changed; it does not interpret meaning.
- **No EffectClass.** Extraction happens iff the caller asks.

## Dependencies

- (no privilege shim — `e2fsck`/`debugfs` are spawned via `Command::new` and the m80 process must already hold `CAP_SYS_ADMIN`; `m80-preflight` verifies at startup).
- `sha2`, `hex` — for base verification.
- `thiserror`, `tracing`, `serde`.

## Tests

- Clone determinism: cloning the same base twice produces byte-identical
  output (verified by sha256).
- Scratch hydration: a workspace tree round-trips through
  `Scratch::create` → boot → exit → `Scratch::extract` and the staged
  set matches the source.
- Admissibility: a workspace containing a symlink, a fifo, and a regular
  file produces a `ChangeSet` with one staged entry and two rejections.
- Swap rollback: with `into` set to a path on a read-only mount, the
  swap fails and the original tree is unchanged.
- e2fsck failure surfacing: a deliberately corrupted image produces a
  typed `E2fsckFailed`.
