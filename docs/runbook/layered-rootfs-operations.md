# Layered Rootfs Operations

Bead: `m80-q420k.5.7`.

This runbook is for operators choosing between plain rootfs overlays, per-VM
pmem layers, same-trust-domain Shared pmem, and snapshot-template warm leases.
Concrete performance numbers come from the committed measurement docs under
`docs/perf/`; do not replace those artifacts with calibration notes.

The Phase E CLI names below are the intended operator surface:

- `m80 image build/list/show/rm/verify`;
- `m80 template build/list/show/prune/rm`;
- existing `m80 warm enable/status/drain/disable`;
- existing `m80 preflight`, `m80 logs`, and `m80 --json env`.

## Rootfs Overlay Choice

Use the shared read-only base rootfs plus per-VM writable overlay as the
default. Phase A optimized only the empty overlay-template clone, not a full
per-VM base-rootfs copy.

Decision tree:

1. If the run-root filesystem supports reflink clone semantics, use the
   reflink overlay-template path.
2. If the host filesystem does not support reflinks, allow the explicit
   byte-copy fallback and budget for the extra clone cost.
3. If an operator sees full base-rootfs copies per VM, treat that as a bug or
   stale configuration. That model is not the target architecture.

Expected operator commands:

- `m80 preflight` to surface host filesystem and run-root advisories.
- `m80 --json env` to capture effective artifact and run-root configuration.
- `m80 logs <vm-id>` when clone or storage preparation fails.

References:

- `docs/behaviors/storage/reflink-rootfs.md`
- `docs/perf/reflink-rootfs-smoke.md`

## Pmem Sharing Decision Tree

Use `PmemSharing::PerVm` when tenants do not share a trust boundary. PerVm gives
each VM its own backing inode even when two declarations name the same digest.

Use `PmemSharing::Shared(TrustDomainAck)` only when the guests are in the same
trust domain. Shared reuses the canonical `m80-image-store` inode to amortize
host page cache, and that deliberately exposes a cache-timing side channel via
shared DAX-backed pages. Cross-tenant isolation between Shared-pmem-sharing
guests is out of scope; the trust-domain assumption is documented in
`docs/positioning.md` "Trust-Domain Assumption for Shared Pmem".

Residual memory-pressure risk is tracked as an operator constraint, not a
runtime mitigation. The verified artifact
`docs/perf/pmem-dax-memory-pressure.md` measured a quiet-host real-KVM run with
two Shared-pmem guests, a 256 MiB uncompressed erofs DAX payload, host
monotonic timing around each guest read command, and a
`stress-ng --vm 1 --vm-bytes 85% --vm-keep --timeout 60s` pressure workload.
It found p50 cross-guest signal delta `0.000 ms`, a settled host
`MemAvailable` drop of `24031440896` bytes during pressure, and zero teardown
residue. Keep using `Shared` only inside a same-trust-domain boundary; do not
add `mlock`, `MAP_POPULATE`, or `madvise(MADV_WILLNEED)` runtime mitigation
from q420k. If a future committed measurement on the target substrate shows a
durable positive signal, file a single-purpose mitigation bead instead of
changing this operator rule inline.

Decision tree:

1. Different tenants or unclear trust boundary: use `PerVm`.
2. Same operator, same namespace, or explicit research sandbox: `Shared` is
   admissible only with `TrustDomainAck`.
3. Any caller asking for a writability hint on a pmem layer is asking for the
   wrong surface. Pmem layers are read-only erofs over virtio-pmem.

Expected operator commands:

- `m80 image verify <digest>` before admitting a layer into the store;
- `m80 image show <digest>` to inspect image size and manifest;
- `m80 image list` for capacity and retention review.

Size limit:

- The pinned Firecracker v1.15.1 virtio-pmem implementation maps each backing
  file through a 512 GiB guest-physical window after rounding the file length to
  2 MiB. m80 rejects an oversize erofs artifact during storage prep, before
  clone, Shared active-use marker creation, jail binding, or Firecracker REST
  admission.
- If a toolchain set grows beyond that limit, split it across multiple
  `PmemLayer` entries. The current API already supports multiple read-only
  layers, bounded by `MAX_PMEM_LAYERS`; a higher-level chunk/recompose helper is
  future tooling rather than VM mechanics.

References:

- `docs/behaviors/rootfs/pmem-layers.md`
- `docs/behaviors/rootfs/pmem-shared.md`
- `docs/decisions/0006-pmemlayer-api.md`
- `docs/positioning.md`

## Template Rebuilds After Upgrades

Snapshot templates fail closed on fingerprint mismatch. A host kernel,
Firecracker, guest kernel, pmem image digest set, post-init state, or hook set
change means the old template is not the same behavior anymore.

Upgrade playbook:

1. Drain warm owners with `m80 warm drain`.
2. Run `m80 preflight` and capture `m80 --json env`.
3. Use `m80 template list` or `m80 template show <fingerprint>` to find
   templates built against the old host/Firecracker/guest inputs.
4. Run `m80 template prune --boot-spec <file>` to remove unpinned templates in
   the same conservative family scope whose fingerprint no longer matches the
   current host/kernel/Firecracker tuple. Remove stale templates outside that
   scope explicitly with `m80 template rm <fingerprint>` after inspection.
5. Rebuild expected templates with `m80 template build ...`.
6. Restart the warm owner with `m80 warm enable ...` and watch restore metrics.

Do not rely on silent rebuild during restore. Fill workers may build a new
template, but restore itself must report the mismatch. A running warm pool does
not hot-revalidate already-ready slots against a new host or artifact tuple; use
drain-and-recreate for host-kernel, Firecracker, guest-kernel, or
BootSpec/template-input upgrades.

References:

- `docs/decisions/0007-snapshot-template-lifecycle.md`
- `docs/behaviors/warm-pool/snapshot-template-build.md`
- `docs/behaviors/warm-pool/snapshot-restore-lifecycle.md`
- `docs/behaviors/warm-pool/vmgenid-reseed-path.md`

## Capacity Planning

Track capacity in three buckets:

- image store: read-only erofs images and rootfs artifacts;
- template store: snapshot template bodies and manifests;
- RAM/page cache: hot rootfs pages, shared pmem pages, guest memory, and VMM
  overhead.

Rules of thumb:

1. PerVm pmem consumes one backing inode per VM and does not claim cross-VM
   page-cache sharing.
2. Shared pmem should make host memory grow like one shared image plus per-VM
   overhead, but the verified density artifact owns the numeric claim.
3. Snapshot-template restore latency and template size must be read from
   committed `docs/perf/` artifacts, not from throwaway PoC numbers.

Expected operator commands:

- `m80 --json image list` to total image-store bytes;
- `m80 --json template list` to total template-store bytes;
- `m80 warm status` to see resident pool sizing;
- Prometheus metric families documented in
  `docs/behaviors/observability/prometheus.md`:
  `m80_image_store_bytes`, `m80_template_store_bytes`,
  `m80_pmem_layers_per_vm_count`, `m80_template_count`, and
  `m80_restore_latency_seconds`.

References:

- `docs/perf/snapshot-template-restore.md`
- `docs/perf/restore-latency.md`
- `docs/perf/measurement-playbook.md`

## Image Store Administration

Images are content-addressed operator inputs. Template eviction does not delete
them. Shared active-use markers are liveness evidence, not ownership or garbage
collection policy.

Expected operator commands:

- `m80 image build` to create or import an artifact through the supported
  image-store path;
- `m80 image verify <digest>` before using a digest in a BootSpec;
- `m80 image show <digest>` for kind, size, and backing path;
- `m80 image rm <digest>` only when no templates or active leases need the
  image.

Before deleting an image, use `m80 image rm <digest>` with the relevant
`--template-store`; the command refuses deletion when committed templates
reference the digest. Also check `m80 warm status`. If in doubt, keep the image
and remove stale templates explicitly first.

References:

- `docs/behaviors/image-build/minimal-erofs.md`
- `docs/behaviors/image-build/manifest-verify.md`
- `docs/behaviors/rootfs/pmem-shared.md`

## Template Store Administration

Templates are immutable bodies addressed by fingerprint. The store supports LRU
eviction with process-scoped pins. Pins are not persistent policy; they only
protect in-flight users inside the current process.

Expected operator commands:

- `m80 template list` to inspect freshness and size;
- `m80 template show <fingerprint>` to inspect fingerprint inputs and
  hook set;
- `m80 template prune --boot-spec <file>` after host, Firecracker, or guest
  kernel changes within one family scope;
- planned `m80 template prune --lru --keep-bytes <n>` for capacity pressure;
- `m80 template rm <fingerprint>` for explicit removal after draining
  warm owners.

Operational rule: drain before destructive template maintenance, prune
invalidated templates after host/kernel bumps, and let the next fill rebuild
visible cache misses.

References:

- `docs/decisions/0007-snapshot-template-lifecycle.md`
- `docs/behaviors/warm-pool/post-restore-hooks.md`
- `docs/behaviors/observability/spans.md`
