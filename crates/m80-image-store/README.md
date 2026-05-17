# m80-image-store

Content-addressed storage for pre-built m80 filesystem images.

This crate is intentionally host-only. It ingests completed artifacts, verifies
their sha256 digest, stores them under a stable digest path, and resolves them
later for callers that need a canonical host file. For erofs artifacts, import
also fails closed on filesystem features outside the pinned m80 guest-kernel
floor so incompatible images do not reach Firecracker admission.

## Contract

- Store root defaults to `/var/lib/m80-images`.
- The root must already exist, be absolute, be a directory, and not be a
  symlink.
- Layout is `<root>/<digest[0..2]>/<digest>/{image.erofs|image.ext4}` with a
  `metadata.json` sidecar.
- `import_existing(source, kind)` is the primary entry point. It hashes a
  pre-built image, copies it into the content-addressed path, and records
  metadata. `ImageKind::Erofs` first runs `dump.erofs -s` and admits only the
  pinned guest-compatible feature floor: `sb_csum`, `mtime`, `0padding`, and
  LZ4/LZ4HC compression if the image declares compression algorithms. Other
  feature or compressor tokens are typed import errors.
- `list()` and `describe(digest)` expose store records without callers walking
  the on-disk layout themselves.
- `resolve(digest)` returns a canonical host path opened with `O_NOFOLLOW`
  when the digest maps to one artifact kind. Use `resolve_as(digest, kind)`
  when the same bytes are stored as multiple kinds.
- `verify(digest)` re-hashes stored bytes on demand. Normal reads trust the
  store path after import.
- `remove(digest)` deletes all artifact kinds for one digest only when no Shared
  active-use markers exist. Snapshot-template reference checks live above this
  crate; the image store does not parse template manifests.
- `acquire_template_build_guard()` and `acquire_gc_execute_guard()` expose the
  shared/exclusive image/template coordination lock used by snapshot-template
  commit and executable image GC. The lock file is
  `<root>/.image-template-coordination.lock`.
- `acquire_shared_ref(digest, vm_id)` creates an active-use marker at
  `<root>/shared/<digest>/refs/<vm_id>` for same-trust-domain shared pmem
  users. `SharedImageRef::release()` removes the marker; `sweep_shared_refs`
  removes stale markers for VM ids that no longer have live run directories.
  These markers never delete canonical content-addressed artifacts.
- Store writes take an exclusive `flock` on `.image-store.lock`; reads take a
  shared lock. The image/template coordination lock is separate from the normal
  store metadata lock so GC can compose template-reference scans with typed
  image removal without changing the store layout.

Opinionated image-build pipelines live outside m80. Nix, mkosi, distro scripts,
and deployment-specific builders should produce `.erofs` or `.ext4` artifacts
and feed them through `import_existing`.

`build_minimal_test_image(source_dir, kind)` is only a small local-dev and test
helper around `mkfs.erofs` / `mkfs.ext4`. It is not a production build system.
The erofs helper emits the default uncompressed erofs feature set and pins
timestamp, uid, gid, worker count, and sort order for byte-identical rebuilds
of simple source trees. The ext4 helper pins UUID and lazy init settings, but
ext4 tooling still has a weaker determinism contract; tests only require that
the produced artifact imports and verifies.

## Public Surface

- `ImageStore` — opens an existing root, imports, lists, describes, resolves,
  verifies, removes, and builds minimal local-dev images.
- `ImageDigest` — lowercase sha256 digest used as the content address.
- `ImageKind` — `Erofs` or `Ext4`.
- `ImageRecord` — digest/kind/path/size record used by list, describe, and
  remove.
- `ImageTemplateCoordinationGuard` — RAII guard for the shared/exclusive
  coordination lock between template build/commit and executable image GC.
- `ImageArtifact` / `ResolvedImage` — enum over resolved artifact types.
- `ErofsImage` and `Ext4Image` — canonical path, byte size, and digest.
- `SharedImageRef` — active-use marker handle for shared pmem artifact users.
- `StoreError` — finite typed error variants. There is no generic string
  fallback.
