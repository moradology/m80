# 0006 — PmemLayer API

## Context

The layered-rootfs warm-pool plan needs a way to attach immutable toolchain or
runtime layers without copying them into every VM rootfs. Firecracker's
virtio-pmem device can expose a host file as a guest block device, and the
Phase 0 PoC under `m80-q420k.7` proved the viable substrate shape for Phase B:
a stripped m80 kernel with built-in erofs, virtio-pmem, libnvdimm, and
filesystem DAX can mount a pmem-backed erofs image with `dax=always` and run
`cargo` from inside the guest.

That proof is deliberately narrow. It proves per-VM pmem-backed erofs layers
can work on real KVM. It does not prove shared host-page behavior, public API
ergonomics, snapshot-template design, or image-build policy. The API therefore
lands in phases:

- Phase B: `PmemLayer` with `PmemSharing::PerVm` only.
- Phase C: `PmemSharing::Shared`, gated by an explicit trust-domain
  acknowledgement and real memory-density evidence.
- Phase D: snapshot-template restore and `WarmStrategy` composition.

This ADR fixes the Phase B vocabulary before implementation so later diffs can
be reviewed against a stable target.

## Decision

Phase B adds a typed pmem-layer surface to the VM mechanics layer:

- `PmemLayer` is one requested read-only pmem mount. It contains an
  `ErofsImageRef`, a `GuestMountPath`, and `PmemSharing::PerVm`.
- `PmemLayers` is the bounded collection admitted by `SandboxConfig`; callers
  cannot pass an unbounded free-form `Vec`.
- `PmemSharing` is a closed enum. Phase B accepts only `PerVm`. The `Shared`
  variant is reserved for Phase C and must fail admission until the trust model
  and density proof land.
- `ErofsImageRef` identifies a verified erofs artifact in the image store by
  digest, not by arbitrary caller path.
- `GuestMountPath` is a validated in-guest destination. Construction rejects
  relative paths, escaping components, kernel pseudo-filesystem destinations,
  and duplicates within one request.
- `ImageDigest` is a validated content digest newtype. It is produced by store
  import/verification paths and compared exactly.

The Firecracker and guest paths stay explicit. Host code wires `PUT /pmem`;
jailer planning binds the selected backing into the jail read-only; guestd
receives a finite `PmemMountRequest` and mounts erofs with the DAX mode that
the real-KVM PoC proved (`dax=always` on the rebuilt stripped kernel). Mismatched
or missing DAX state fails closed.

The image store is polymorphic. `m80-image-store` owns a single
content-addressed store whose artifacts are sibling enum variants:
`ImageArtifact::Erofs` and `ImageArtifact::Ext4`. Consumer-facing pmem layers
still name `ErofsImageRef` because pmem layer mounting is erofs-specific in
Phase B, but store internals are not split into `m80-image-erofs`.

m80 ingests pre-built `.erofs` and `.ext4` artifacts and verifies digests.
Opinionated build systems such as Nix, mkosi, distro-specific scripts, or
operator CI pipelines live outside m80. A minimal `build_minimal_test_image`
helper may exist for tests and local development only.

`WarmStrategy` does not land in Phase B. Snapshot-template selection,
fingerprints, post-restore hooks, and template store policy are Phase D work.
Phase B types must leave room for those fields without pretending snapshot
restore exists.

`TrustDomainAck` is named now but not implemented in Phase B. Shared pmem is a
cross-VM data-sharing and timing-surface choice. Phase C must make the caller
acknowledge that trust domain explicitly before `PmemSharing::Shared` becomes
admissible.

## Implementation Checklist

Phase B implementation PRs that touch caller-controlled values flowing to
kernel-facing primitives must quote and tick the AGENTS/CLAUDE
untrusted-input checklist for:

- kernel cmdline tokens;
- mount source and destination paths;
- cgroup or other virtual-file payloads if a later diff introduces them.

Phase B PRs that touch mount flags, jailer planning, Firecracker pmem calls, or
smoke scripts are not audit-sweep eligible and need real-KVM smoke evidence per
ADR 0003 and the 2026-05-12 postmortem.

## Consequences

The first production slice is smaller: callers can attach per-VM erofs layers,
but cannot opt into shared backing until Phase C. That is intentional. It lets
Phase B prove typed admission, jail binding, FC pmem wiring, guest mount
semantics, and cleanup without also settling the side-channel story.

The polymorphic image store keeps artifact lifecycle in one place. It prevents
a premature split between erofs and ext4 stores while preserving type safety at
the pmem mount boundary.

The API is not backward-compatible with hypothetical older shapes. m80 is
pre-1.0 and has a closed call graph; hard cutover is the right failure mode for
incorrect internal assumptions.

The PoC also showed that VMGenID is not guest-visible in the current stripped
kernel profile because ACPI is disabled. That does not change Phase B, but it
does constrain Phase D: post-restore hooks use a host-driven restore signal and
explicit userspace reseed unless a future kernel profile enables
`CONFIG_VMGENID=y`.

## Phase C Shared Bind Update

`PmemSharing::Shared` now uses a distinct jailer bind mode:
`BindMode::RoImageStore`. The mode is still a read-only bind, but it adds a
source-policy backstop: `Plan::compute` rejects sources that are not rooted
under `/var/lib/m80-images`, and materialization rechecks the canonical source
against the canonical image-store root before the bind mount. This keeps the
shared path constrained to m80-owned, digest-addressed artifacts instead of
letting an arbitrary caller path ride the shared-pmem lane.

The O_RDONLY assertion for Phase C uses the guest-observable route rather than
host-side Firecracker fd inspection. The ignored real-KVM test
`pmem_layer_shared_reuses_backing_inode_real_kvm` launches two VMs with one
shared backing, proves both jail paths point at the same image-store inode, and
asserts that each guest `/proc/mounts` line for the layer includes both `ro`
and DAX. This is the contract consumers observe: the shared erofs layer is
mounted read-only inside every guest while retaining the shared host inode.

Shared active-use tracking uses on-disk markers under the image store:
`<store>/shared/<digest>/refs/<vm_id>`. These markers are ref-like liveness
evidence, not ownership of the canonical artifact. The image artifact itself is
operator-managed and remains in the content-addressed store after the final VM
releases its marker. This is a deliberate correction to the early Phase C
wording that implied deletion at refcount zero: deleting the canonical
image-store input would make future launches fail and would conflate active-use
tracking with garbage collection. Any future artifact GC must be an explicit
operator action with its own retention policy.

Storage prep acquires a marker for every `Shared` layer before launch returns a
running VM. Stop and force-kill release markers only after the jail has been
dropped, so the bind mount is gone before the marker disappears. Backend
startup sweeps stale markers for VM ids that no longer exist under the run
root, covering process-crash residue without silently deleting artifacts.

## Alternatives Considered

**Free-form `pmem_layers: Vec<PmemLayer>`:** rejected. A bounded collection
newtype gives admission one owner for maximum layer count, duplicate mount
destinations, and future FC device-slot limits.

**`GuestMountPath` as `String`:** rejected. Guest destinations feed a kernel
mount primitive; validation must happen at typed construction, not at the final
consumer.

**Ship `PmemSharing::Shared` in Phase B:** rejected. Shared backing needs a
trust-domain acknowledgement, O_RDONLY bind assertions, density evidence, and
side-channel documentation. Those are Phase C gates, not Phase B scope.

**Collapse rootfs, scratch, and pmem under a polymorphic `MountSpec`:**
rejected for Phase B. Rootfs and scratch are exactly-one lifecycle surfaces with
Firecracker-specific plumbing; pmem layers are zero-or-many, read-only, and
guest-mounted. Future composition can add a higher-level enum without changing
the lower-level types.

**Ship an opinionated image-build pipeline:** rejected. m80 owns VM mechanics:
artifact admission, digest verification, and launch wiring. Build systems live
in operator or adapter infrastructure.

## Phase G Recheck: MountSpec Shape

Bead `m80-q420k.8.1` rechecked the earlier `MountSpec` question after Phases
B, C, and D had shipped. The decision remains unchanged: keep rootfs, scratch,
and pmem as distinct lower-level surfaces. The live implementation still gives
rootfs and rootfs overlay fixed Firecracker drive ids and exactly-one lifecycle
roles; workspace scratch is optional, writable, tied to writeback/extraction
semantics, and may be omitted for warm slots; pmem layers are a bounded
zero-or-many read-only list validated as `PmemLayer`, resolved through the
image store, attached as virtio-pmem, and guest-mounted after readiness.
`BootSpec` mirrors that split with `sandbox` fields plus a separate
`pmem_layers` array rather than a polymorphic mount list. The current Torpor
adapter also wants the split: it configures rootfs as deployment artifact,
workspace as the per-request guest workspace/writeback surface, and warm
workspace drives as adapter-owned lease state; it has no consumer pressure for
a single enum. If a later adapter wants a unified presentation, add it as an
additive config/view layer that lowers into these existing types, not by
collapsing the lifecycle primitives.

## Phase G Recheck: Pmem Device Size

Bead `m80-q420k.8.4` checked Firecracker v1.15.1 rather than assuming the
limit shape. The API schema and pmem user doc do not expose a configured
per-device maximum, but the implementation maps the backing file, rounds the
region up to 2 MiB, and allocates that region from `past_mmio64_memory`.
Both x86_64 and aarch64 define that post-MMIO window as 512 GiB in v1.15.1.
The attach path currently unwraps the address allocation, so m80 must reject
oversize artifacts before handing them to Firecracker.

The validation belongs in storage prep, not `PmemLayer::new`: the public layer
type carries an image digest and sharing/mount policy, while the byte length is
known only after `m80-image-store` resolves the digest to an erofs artifact.
Storage prep now rejects artifacts whose Firecracker-rounded backing length
would exceed the 512 GiB window before clone creation, Shared marker creation,
jail binding, or REST admission. Workloads larger than one pmem device should
split across multiple declared layers, still bounded by `MAX_PMEM_LAYERS`; a
future helper can automate chunking and mount-time recomposition without
changing the VM-mechanics surface.

## References

- `docs/poc/2026-05-16-layered-rootfs-poc-findings.md`
- `docs/behaviors/image-build/pmem-erofs-dax-kernel.md`
- `docs/behaviors/warm-pool/vmgenid-reseed-path.md`
- `docs/decisions/0003-audit-sweep-eligibility.md`
- `docs/postmortems/2026-05-12-ms-bind-and-sudo-escape.md`
