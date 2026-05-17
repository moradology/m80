# `m80-snapshot-template`

Content-addressed snapshot-template store for Phase D warm pools.

The crate stores reusable Firecracker snapshot bodies by
`TemplateFingerprint`. It owns the persisted manifest, index, atomic publish
path, and process-local pins. It does not launch Firecracker, decide when to
build a template, bind paths into a jail, run post-restore hooks, or delete
`m80-image-store` artifacts referenced by a template.

## On-disk layout

`TemplateStore::create(root, capacity)` initializes:

```text
<root>/
  index.json
  staging/
  by-fingerprint/
    <fingerprint-hex>/
      vm.snap
      mem.snap
      manifest.json
      snapshot-manifest.json
```

Builds are written under `staging/` first. `TemplateStore::commit` requires both
`vm.snap` and `mem.snap`, computes the `m80-snapshot` manifest, writes both
`manifest.json` and the restore-side `snapshot-manifest.json`, and only then
renames the staging directory into `by-fingerprint/<hex>/`. The snapshot
manifest records the final content-addressed `vm.snap` and `mem.snap` paths,
not the temporary staging paths. A missing body file never creates a visible
template.

## Template identity

`TemplateFingerprint::compute` hashes the ADR 0007 tuple:

- host kernel version
- Firecracker version
- guest kernel digest
- deterministic pmem image digest set, including guest mount path, image
  digest, sharing mode, and stable jail-visible backing path
- post-init state digest
- ordered post-restore hook-set digest

`TemplateInputs` are serialized into `manifest.json`. Store reads recompute the
fingerprint from those typed inputs and compare it with the live inputs supplied
by the caller. A mismatch returns
`TemplateStoreError::FingerprintMismatch { stored, live }`; the store never
silently restores or rebuilds a mismatched template.

## Pins and eviction

`TemplateStore::pin` and cache-hit `lookup` return `PinnedTemplate`. The pin is
process-local RAII: keeping the `PinnedTemplate` alive keeps the template body
unevictable in that process, and dropping it releases the pin. Pins are not
persisted across process restart.

`evict_lru_to_capacity` removes least-recently-used unpinned template
directories. `evict_invalidated` removes unpinned templates that do not match a
new live input tuple. Template eviction removes only the snapshot-template body
directory under this store. It never deletes `m80-image-store` artifacts; image
artifacts are operator-managed inputs.

`evict_invalidated_in_scope` is the operator-safe prune helper. It removes only
unpinned templates whose manifest has the same pmem image set, post-init state
digest, and hook set as the supplied live inputs, but a different fingerprint.
That prunes host-kernel, Firecracker-version, or guest-kernel invalidations for
one template family without sweeping unrelated families from the same store.

`templates_referencing_image(image_digest)` reads committed manifests and
returns the template fingerprints that include that pmem image digest. This is a
read-only operator guard for image-store deletion; it does not make templates
own or garbage-collect image artifacts.

`manifest(fingerprint)` reads one committed manifest for operator inspection.
`remove(fingerprint)` deletes one unpinned committed template and updates the
index. Process-local pins fail closed with `TemplateStoreError::TemplatePinned`;
cross-process lease coordination remains the caller's responsibility.

## Public surface

- `TemplateStore::{create, open, lookup, reserve, commit, pin, manifest,
  remove, evict_lru_to_capacity, evict_invalidated,
  evict_invalidated_in_scope, list, templates_referencing_image}`.
- `TemplateBuildPlan` with staging body paths for `vm.snap` and `mem.snap`.
- `PinnedTemplate`, `TemplateRef`, and `TemplatePin`.
- `TemplateSummary` for index-backed list output.
- `TemplateManifest`, `TemplateRestoreLayout`, `TemplateInputs`,
  `TemplateFingerprint`, `TemplateDigest`, `PmemTemplateEntry`,
  `PmemTemplateSharing`, `ImageDigest`, `GuestMountPath`, `JailBackingPath`,
  `HookSpecSet`, `HookSpec`, and `HostnameSpec`.
- `Index` and `IndexEntry` for the persisted LRU index schema.
- `TemplateStoreError` with typed failure variants, including
  `TemplatePinned` for process-local removal refusal.

## Tests

`tests/store.rs` covers cache miss/reserve/commit/cache hit, incomplete-body
commit failure, pin-aware LRU eviction, fail-closed fingerprint mismatch plus
invalidated eviction, and the index schema-version probe/unknown-field
behavior.
