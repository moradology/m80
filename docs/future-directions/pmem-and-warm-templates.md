# Pmem Layers And Warm Templates

This note tracks the layered-rootfs warm-pool direction behind `m80-q420k`.
It is intentionally forward-looking; implementation authority stays in the
phase beads and crate READMEs.

## Recap

The design combines:

- a shared read-only base rootfs plus per-VM writable overlays cloned from a
  run-root-local overlay template;
- read-only erofs layers attached through Firecracker virtio-pmem;
- optional shared backing for trusted same-domain VMs;
- snapshot-template restore for low-latency warm leases;
- typed post-restore hooks for per-lease uniqueness.

The Phase 0 PoC proved the core substrate is viable on real KVM once the
stripped kernel keeps erofs, virtio-pmem, libnvdimm, and filesystem DAX built
in. It also proved that Firecracker preserves a pmem-backed erofs mount across
snapshot restore when the backing path is stable.

## Phasing

- Phase A: explicit clone policy for the empty writable overlay template, with
  reflink and byte-copy modes plus a probe-only auto selector.
- Phase B: `PmemLayer` with `PmemSharing::PerVm` and erofs over virtio-pmem.
- Phase C: `PmemSharing::Shared` behind explicit trust-domain acknowledgement.
- Phase D: snapshot-template restore, template fingerprints, and post-restore
  hooks.
- Phase E: CLI/config/observability/runbook surfaces for the new mechanics.
- Phase F: composed real-KVM e2e and verified measurement artifacts.

## Phase B Scope

Phase B is deliberately PerVm-only. It proves typed admission, content-addressed
artifact resolution, jail read-only binding, Firecracker `PUT /pmem`, and guest
erofs+DAX mount semantics without making cross-VM sharing claims.

Phase B does not ship `WarmStrategy`, template fingerprints, shared trust
domain acknowledgement, or operator-facing image-build pipelines.

## Resolved And Open Questions For Phase C/D

Phase C must decide the exact `TrustDomainAck` shape and the caller-visible
language around DAX timing/cache side channels. Shared backing cannot become
admissible until that acknowledgement, O_RDONLY binding checks, and committed
real-KVM density evidence exist.

Phase C/F resolved the compressed-erofs layout question in
`docs/perf/erofs-dax-sharing-layout.md`: only uncompressed non-inlined erofs
files receive guest-visible `STATX_ATTR_DAX` on the current real-KVM
substrate. Compressed erofs files still mount with `dax=always`, but they are
not valid inputs for Shared-pmem density proofs. Shared storage prep now
rejects compressed erofs artifacts before active-use marker creation, jail
materialization, or Firecracker admission.

Phase D template fingerprints must include enough host and guest facts to fail
closed after upgrades: host kernel version, Firecracker version, guest kernel
identity, pmem image digest set, post-init state digest, and hook-set digest.

Phase D post-restore uniqueness currently uses a host-driven restore signal
with a fresh host-generated restore nonce plus explicit guest
`RNDRESEEDCRNG`. The current stripped kernel disables ACPI, so Linux
`CONFIG_VMGENID` cannot consume Firecracker VMGenID. A future
ACPI/VMGenID-capable kernel profile is tracked separately.

## Phase E Ergonomics

The CLI should expose image and template operations only after the underlying
mechanics are stable. Expected gaps:

- inspect which erofs/ext4 artifacts are present and verified;
- show template fingerprints and invalidation reasons;
- explain whether a lease is cold, per-VM pmem, shared pmem, or
  snapshot-restored;
- surface restore latency, hook duration, and density metrics without turning
  calibration numbers into verified perf claims.

## Guardrail

Do not expand this direction into new abstractions without a concrete consumer
or a closed phase bead that needs the abstraction. The point is to preserve the
path, not to build a framework ahead of evidence.
