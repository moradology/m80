# 0009 - Image Store GC

## Context

`m80-image-store` owns content-addressed rootfs and pmem image artifacts under
`<root>/<digest[0..2]>/<digest>/{image.erofs|image.ext4}` with a metadata
sidecar. The store is already sharded by digest prefix; the earlier "flat
directory" concern no longer matches the live layout.

Images are operator-managed inputs, not cache entries. They are referenced by:

- committed snapshot-template manifests through each template's
  `pmem_image_digest_set`;
- active Shared pmem VMs through
  `<image-store>/shared/<digest>/refs/<vm_id>` markers;
- external operator config such as BootSpecs, deployment manifests, or image
  pin lists that m80 cannot discover by walking its own stores.

The existing `m80 image rm <digest>` path is explicit and single-digest. It
opens the configured template store, refuses removal when a committed template
manifest references the digest, and then calls `ImageStore::remove`, which
refuses removal while Shared active-use markers exist.

## Decision

Image GC is an operator-driven mark-and-sweep command, not a background daemon
and not an automatic disk-pressure response in v0.x. The first implementation
should add `m80 image gc` with a dry-run plan as the default and an explicit
`--execute` mode for deletion.

The GC mark set is the union of:

- image digests referenced by committed templates in the configured
  `TemplateStore`;
- image digests with active Shared pmem markers;
- operator pins supplied as a file or repeated CLI flag;
- optional recency retention, expressed as a minimum artifact age before
  deletion eligibility.

Everything outside that mark set is only a candidate. Deletion still reuses the
same typed removal path as `m80 image rm`, so one bad candidate cannot bypass
the Shared-marker or template-reference checks.

The implemented CLI keeps report-only mode as the default. `m80 image gc`
renders the candidate/protected plan and `total_reclaimable_bytes`; `--execute`
is the only mode that removes candidates.

GC must not infer unused images from running VMs alone. PerVm pmem launches
clone the erofs artifact into the run directory before Firecracker admission,
so a live PerVm VM does not require the store artifact for its current
lifetime. Shared pmem does require the canonical store inode, and that is
tracked by active-use markers.

## Concurrency

The implementation must not delete an image between a template build deciding
to reference the image and the template commit becoming visible. A plain
"scan templates, then remove image" loop is racy because template commits and
GC live in separate stores.

Executable GC uses a shared image-store coordination lock:

- snapshot-template cache misses that will publish manifests referencing
  image-store digests hold `<image-store>/.image-template-coordination.lock` in
  shared mode for the build/commit window;
- `m80 image gc --execute` holds the same lock in exclusive mode while scanning
  template references and deleting candidates.

The lock is separate from `.image-store.lock`, which continues to protect the
image-store layout and Shared active-use markers. The lock order is
coordination lock first, then ordinary image-store operations. Template commit
does not take the ordinary image-store lock, so this order does not create a
cycle with template-store commit.

`m80 image rm` remains explicit and single-digest. It keeps the same
template-reference and Shared-marker checks but does not take the GC
coordination lock; it is for operators who intentionally remove a known digest.

## Retention Policy

The first executable GC should support conservative knobs only:

- `--pin-file <path>`: newline-delimited image digests to keep;
- `--keep <digest>`: repeated one-off keep rules;
- `--min-age <duration>`: do not delete artifacts whose metadata is newer than
  this age;
- `--template-store <path>`: committed templates whose manifests protect image
  digests;
- `--execute`: required to delete. Omitted means report-only.

Do not add a background daemon, disk-usage threshold trigger, or automatic
default deletion policy in v0.x. Operators can run the explicit command from a
scheduler once its report output is stable.

## Consequences

The image store stays the owner of artifact bytes and Shared active-use
markers. The snapshot-template store stays the owner of template manifests,
pins, and template eviction. GC composes the two stores at the CLI/operator
layer because neither crate alone owns the complete reference graph.

The current sharded path layout is sufficient for the v0.x horizon. A future
layout migration is only justified by measured filesystem pressure in the
existing `<digest[0..2]>/<digest>` layout, not by the old flat-directory
assumption.

Template eviction never deletes images. Image GC never deletes templates. The
operator order remains: prune or remove stale templates first, then run image
GC to remove now-unreferenced artifacts.

## Alternatives Considered

**Background image GC:** rejected for v0.x. It can delete operator inputs at
surprising times and needs cross-store coordination before it is safe.

**Disk-threshold-triggered deletion:** rejected for the first implementation.
Disk pressure should produce explicit operator action until the plan output,
pinning model, and race prevention have had production use.

**Put GC entirely inside `m80-image-store`:** rejected. The image store can see
active Shared markers, but it cannot parse template manifests or external
deployment pins without taking dependencies that would invert crate ownership.

**Treat only active Shared markers as references:** rejected. Committed
snapshot templates need the same image digests to be present at restore time.

## References

- `m80-q420k.8.15`
- `crates/m80-image-store/src/store.rs`
- `crates/m80-cli/src/cmds/image.rs`
- `crates/m80-snapshot-template/src/store.rs`
- `docs/decisions/0007-snapshot-template-lifecycle.md`
