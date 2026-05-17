# 0007 - Snapshot Template Lifecycle

## Context

Phase D of `m80-q420k` turns m80's existing Firecracker snapshot/restore
primitive into reusable warm-pool templates. The existing
`docs/design/snapshot-restore.md` locks the lower-level Firecracker behavior:
snapshot files, vsock reset semantics, jail-visible snapshot paths, and the
restore probe. This decision record adds the template layer above that
primitive: identity, storage, invalidation, eviction, and post-restore
uniqueness hooks.

The Phase 0 PoC changed the shape of this phase. It proved that Firecracker
v1.15.1 preserves a pmem-backed erofs DAX mount across snapshot/restore when
the backing path is stable, so snapshot templates may include pmem layers from
Phase B/C. It also proved that Firecracker can emit VMGenID updates while the
current stripped m80 guest kernel cannot consume them: the stripped profile has
ACPI and PCI disabled, and Linux 6.1.134 `CONFIG_VMGENID` depends on ACPI.
Therefore Phase D cannot wait for a guest-visible VMGenID counter. The v0.1
path is host-driven: after restore, the host sends a typed hook request with
fresh host entropy, and guestd runs a fail-closed reseed and hook sequence
before the lease is handed back.

This path is kernel-touching once implemented. Snapshot restore, pmem device
state, jail path materialization, random reseed, hook sequencing, and smoke
scripts all need real-KVM evidence. Measurement-shaped children, especially
restore latency, close only with committed real-substrate artifacts per ADR
0002.

## Decision

Phase D introduces snapshot templates as typed, fingerprinted artifacts. A
template is not just a raw `vm.snap`/`mem.snap` pair. It is:

- a Full Firecracker snapshot pair;
- a typed template manifest;
- the typed set of required pmem/image-store artifacts;
- the post-init state digest that says what guest state was captured;
- the closed hook set that will run after every restore;
- enough layout metadata for restore to recreate the same jail-visible backing
  paths Firecracker captured in the snapshot.

### Template Fingerprint

`TemplateFingerprint` is versioned. `TemplateFingerprintV1` is the hash of the
following typed tuple:

```text
(
  host_kernel_version,
  firecracker_version,
  guest_kernel_digest,
  pmem_image_digest_set,
  post_init_state_digest,
  hook_spec_set_digest,
)
```

The tuple contains typed values, not caller-provided prose.

`host_kernel_version` is the host kernel release that the template was built
under. `firecracker_version` is the exact Firecracker version used to capture
the snapshot. `guest_kernel_digest` identifies the guest kernel image, not just
the guest kernel version string.

`pmem_image_digest_set` is deterministic. It is an ordered set of canonical
layer entries sorted by guest mount path, then image digest, then sharing
mode. Each entry records:

- `GuestMountPath`;
- `ImageDigest`;
- `PmemSharing` mode;
- the stable jail-visible pmem backing path shape required at restore.

For `PmemSharing::Shared`, the trust-domain acknowledgement is still required
at admission time, but the template fingerprint records the sharing mode and
image digest, not a free-form trust string. Phase C owns the trust doctrine and
same-inode density proof.

`post_init_state_digest` is a sha256 over a typed post-init snapshot manifest.
It covers the guest state that template build intentionally captures after
boot and pmem mount setup, before any per-lease workload. It must not be a
hash of ad hoc logs.

`hook_spec_set_digest` is a sha256 over the canonical encoded hook list. Two
templates with the same guest and pmem state but different per-restore hooks
are different templates because the lease hand-back contract is different.

### Template Store

The snapshot-template store is content-addressed by `TemplateFingerprint`.
The template body contains the snapshot pair and typed manifest. The image
store remains the owner of pmem artifacts; template eviction never deletes
canonical image-store artifacts.

Template bodies are immutable once written. Rebuilding the same fingerprint
must either verify the existing body or fail with a typed corruption/conflict
error. It must not overwrite in place.

The store supports LRU eviction with a process-scoped `pin()` API. A pin keeps
a template body from being removed while the process holds the pin. Pins are
not persisted across process restart. On startup, the store may recover stale
pin metadata by ignoring it; pinned state is process memory, not durable
policy.

### Invalidation

Invalidation is fail-closed. If a requested template fingerprint mismatches
the live host, Firecracker binary, guest kernel, pmem layer set, post-init
state, or hook set, restore returns a typed mismatch error. It does not
silently restore and it does not silently rebuild.

The fill worker owns rebuild. A mismatch can cause the fill worker to build a
new template, but that is a visible fill operation, not a side effect of a
lease restore.

### Host Upgrade Cutover

Bead `m80-q420k.8.6` rechecked whether a running warm pool should serve old
ready slots while rebuilding templates after a host upgrade. The v0.x answer is
no: drain and recreate the pool on host-kernel, Firecracker, guest-kernel, or
BootSpec/template-input changes.

`WarmPool` has immutable backend discovery and immutable `WarmStrategy` after
construction. A ready slot is validated by the fill path that created it; the
lease checkout path is allocation only and does not revalidate the slot against
a newly discovered host/kernel/Firecracker tuple. Trying to support
serve-old-while-rebuilding would require versioned ready queues, per-slot
template fingerprint tracking, and an invalidation-severity policy that can
prove which old slots are safe to serve. That is broader than the v0.x
mechanics surface.

The supported cutover is:

1. stop admitting new leases through the old warm owner;
2. drain or discard old ready slots;
3. restart or recreate `Backend` and `WarmPool` after preflight sees the new
   host and artifact tuple;
4. let the fill worker miss old fingerprints and build the new template
   explicitly;
5. optionally run `m80 template prune --boot-spec <file>` to remove old
   unpinned templates in the same conservative family scope.

After process restart, the old template can remain in the store without being a
cache hit: `TemplateFingerprint::compute(live_inputs)` changes, `lookup`
misses, and the fill worker builds the current template. No
`TemplateInvalidationSeverity` enum lands in v0.x; if a real deployment later
needs rolling hot reload, that should be a separate design with explicit
per-input severity and slot-versioning semantics.

### Restore Layout

Firecracker snapshots record device paths and device state. The restore path
therefore recreates the captured jail-visible path contract before
`PUT /snapshot/load`.

For pmem layers, restore materializes the same stable jail-visible backing
paths that template capture used. For `PerVm`, that may mean a per-slot clone
or per-slot backing file behind the same jail path. For `Shared`, it means a
read-only bind to the canonical image-store artifact behind the same jail path.

Mutable per-lease state such as scratch must be fresh for every lease, but it
still has to satisfy the path/device contract captured in the snapshot. The
v0.1 warm-pool strategy keeps the existing one-use lease model: fill work
creates the fresh slot backing, restores the template, runs hooks, and only
then places the slot in the ready queue. `WarmPool::try_lease` is allocation
only. Because a leased slot is discarded instead of reused, fill-time restore is
the per-slot and per-lease freshness boundary. It must not hand a lease back
with mutable state from the template build VM.

### Post-Restore Hooks

Post-restore hooks are host-driven. The host sends
`PostRestoreHookRequest` over the guest control channel after snapshot load
and resume, and before lease hand-back. The request contains:

- `restore_nonce`: 32 bytes generated by the host with `getrandom()` for this
  single restore;
- `hooks`: a canonical ordered list of closed `HookSpec` variants.

The restore nonce is host-generated m80 data, not caller input. Guestd mixes it
into the guest random pool before forcing a reseed. This avoids the weak shape
where multiple cloned guests ask an identical snapshotted entropy pool to
reseed itself without fresh input.

The initial closed hook enum is:

```rust
enum HookSpec {
    ReseedSystemdRandomSeed,
    RegenMachineId,
    SetHostname(HostnameSpec),
}
```

`HostnameSpec` validates RFC-1123 form and length at construction.

The hook executor runs in this order:

1. Mix `restore_nonce` into `/dev/urandom`.
2. Call `ioctl(RNDRESEEDCRNG)` on `/dev/urandom` for the current stripped
   kernel profile.
3. Run the requested typed hooks in request order.
4. Return `PostRestoreHookResponse` with typed success or failure details.

If `/var/lib/systemd/random-seed` exists and `ReseedSystemdRandomSeed` is
requested, guestd rewrites it from fresh guest random output after the kernel
reseed. Missing systemd random-seed state is normal for minimal guests and is
not an error unless the hook variant is later split into a strict systemd-only
variant.

Any reseed or hook failure rejects the lease. The host tears down the restored
VM and does not return a half-unique or partially initialized VM to the caller.

The host applies one aggregate response deadline to the post-restore hook RPC.
The deadline covers nonce mix, `RNDRESEEDCRNG`, and the ordered hook list. A
timeout is fail-closed and maps to the same protocol read-timeout class as
other missing terminal frames; no later hook is assumed to have succeeded, and
the restored VM is torn down instead of handed to the caller. The v0.1 policy
is a fixed 5 second aggregate deadline, not a per-hook caller knob. Per-hook
timeouts would require extending the wire contract and error taxonomy, and
the current closed hook set is bounded filesystem/syscall work that should
complete far below the restore-latency budget on a healthy guest.

The current stripped profile does not use guest-visible VMGenID. If
`m80-q420k.4.16` later adopts an ACPI/CONFIG_VMGENID-capable snapshot kernel,
that branch must prove the kernel path with real KVM and update this contract
before any code relies on it.

### Closed Hook Surface

`HookSpec` is a closed enum and must not use `#[non_exhaustive]` in these
pre-1.0 internal crates. Adding a hook variant should create compile errors at
every executor and mapper that needs to understand the new behavior.

`HookSpec::RunPostRestoreUserCmd` is rejected for v0.1. A free-form command
would push caller-controlled argv, environment, path, and failure semantics
into a privileged guest lifecycle boundary. That is the opposite of the
typed-API-boundary rule in the untrusted-input-to-kernel-primitive checklist.
`AGENTS.md` also forbids `#[non_exhaustive]` on internal-only pre-1.0 enums;
the compile errors from exhaustive matches are the desired review signal here.
If a real caller later needs custom hook behavior, it gets a separate typed
API with explicit validation and its own security review.

## Consequences

Template identity becomes stricter than a raw snapshot filename. That is
intentional. A restore from a stale host, stale Firecracker binary, stale guest
kernel, changed pmem layer set, changed post-init state, or changed hook set is
a different behavior and must not look like a cache hit.

The template store does not become image garbage collection. Image-store
artifacts are operator-managed inputs, and Shared active-use markers from
Phase C are liveness evidence, not ownership of the canonical artifact.

The host-driven nonce and `RNDRESEEDCRNG` path keeps Phase D unblocked on the
current stripped kernel while still leaving room for a future VMGenID-capable
kernel profile. That future branch is a kernel-profile change, not a silent
implementation detail.

LRU eviction with process-scoped pins is simple enough for v0.1. Durable pins,
multi-host template distribution, signing, and retention policy are future
operator features, not hidden requirements for the first template store.

## Alternatives Considered

**Use raw snapshot paths as template identity:** rejected. Raw paths do not
encode host kernel, Firecracker, guest kernel, pmem layers, post-init state, or
hook behavior. They would let stale snapshots masquerade as valid templates.

**Silently rebuild on mismatch:** rejected. Rebuild is real work and may hide a
broken cache or host rollout. Restore fails closed; fill workers decide when to
build.

**Persist pins:** rejected for v0.1. Persistent pins become retention policy
and need operator UX. Process-scoped pins are enough to keep in-flight leases
safe.

**Rely on Firecracker VMGenID for v0.1:** rejected. Firecracker logs VMGenID
updates, but the current stripped guest kernel cannot consume them. The v0.1
contract is host-driven reseed and typed hooks.

**Run arbitrary post-restore commands:** rejected. Phase D hooks are lifecycle
operations with privileged guest effects. Closed typed variants keep validation
and failure handling explicit.

**Let templates delete image-store artifacts on eviction:** rejected. Image
artifacts are shared operator-managed inputs. Template eviction only removes
template bodies.

## References

- `docs/design/snapshot-restore.md`
- `docs/poc/2026-05-16-layered-rootfs-poc-findings.md`
- `docs/behaviors/warm-pool/vmgenid-reseed-path.md`
- `docs/future-directions/pmem-and-warm-templates.md`
- `AGENTS.md`
- `docs/decisions/0002-bead-closure-scaffolded-vs-verified.md`
- `docs/decisions/0003-audit-sweep-eligibility.md`
- `docs/postmortems/2026-05-12-ms-bind-and-sudo-escape.md`
