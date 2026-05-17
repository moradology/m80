# Pmem Layer Splitting

Bead: `m80-q420k.8.14`

Firecracker v1.15.1 exposes a finite virtio-pmem address window. m80 rejects a
single erofs backing that would exceed the pinned window after Firecracker's
2 MiB rounding. Operators can already declare multiple independent
`PmemLayer`s, but m80 has no helper that turns one large source set into a
bounded set of erofs layers with a predictable guest composition contract.

This is future tooling, not new VM mechanics. The existing rootfs, scratch,
pmem, and snapshot-template contracts should stay distinct.

## Split Unit

The split unit should be a path-prefix group inside an input source set:

```text
source-set/
  toolchain/
  registry-cache/
  docs/
  model-shards/
```

The helper chooses groups that each fit under the Firecracker pmem backing
limit and the configured `MAX_PMEM_LAYERS` budget. It should prefer stable
semantic directories over byte-range chunking. Splitting one regular file into
multiple pmem images is not a v0.x target because it requires a guest-side
reassembly mechanism and weakens the "mount read-only erofs" contract.

If a single source file or directory cannot fit in one pmem backing, the helper
should fail with an explicit "cannot split this source set with path-prefix
policy" error rather than inventing a new runtime filesystem.

## Manifest Shape

A future helper can emit a manifest like:

```text
PmemLayerSetManifest {
  schema_version,
  source_set_digest,
  split_policy,
  layers: [
    {
      name,
      image_digest,
      mount_at,
      source_prefixes,
      size_bytes,
      erofs_layout,
    },
  ],
  guest_contract,
}
```

Each `image_digest` is still an ordinary `m80-image-store` erofs digest. The
helper does not create a new store namespace. Existing callers that already
declare independent `PmemLayer` entries continue to work without this manifest.

The manifest should record:

- source-set digest over the complete input before splitting;
- split policy and deterministic ordering;
- per-layer erofs digest and size;
- guest mount path for each layer;
- any caller-facing environment or config snippet needed to consume the set.

## Guest Contract

The conservative guest contract is "many mounted directories." The helper emits
BootSpec layer entries such as:

```text
/opt/m80-layers/toolchain-0
/opt/m80-layers/toolchain-1
/opt/m80-layers/toolchain-2
```

Consumers compose them by explicit config: `PATH`, `LD_LIBRARY_PATH`, language
package search paths, or application-specific manifests. m80 should not add
guest overlayfs, FUSE, bind-mount fanout, or symlink-farm mutation just to make
multiple layers look like one directory.

For toolchains, the best split is usually by already-independent subtrees:
compiler, sysroot, registry/cache, docs, and optional large model or dataset
shards. For datasets, the split should follow the dataset's own shard index.

## Interaction With Shared Pmem

Layer splitting composes with `PmemSharing::PerVm` and
`PmemSharing::Shared(TrustDomainAck)`. Shared layers still require the explicit
same-trust-domain witness, and Shared density claims still require the
`.8.12`-valid erofs layout for file-level DAX. A future splitting helper should
therefore be able to request uncompressed non-inlined erofs output for Shared
density-critical payloads.

## Non-Goals

- No new Firecracker pmem device model.
- No byte-range split/reassembly of one regular file.
- No guest overlayfs or FUSE composition layer.
- No automatic collapse of pmem layers into rootfs or scratch.
- No bypass of `m80-image-store` erofs compatibility validation.
- No kernel-facing diff without a single-purpose bead and real-KVM smoke
  evidence.

## Reopen Conditions

Implement the helper when an operator has a concrete source set that:

- exceeds the single-backing limit;
- can be split by stable path prefixes;
- fits within the remaining pmem layer count budget;
- needs a reproducible manifest rather than hand-written BootSpec layers.

Until then, independent `PmemLayer` declarations are the contract.
