# Multi-Host Template Distribution

Bead: `m80-q420k.8.10`

Snapshot-template storage is single-host in v0.x. That is intentional. A
template is fingerprinted from the host kernel, Firecracker binary, guest
kernel, pmem image digest set, post-init state, and hook set. A cache hit on a
different host is valid only when those inputs match exactly and the template
body can be verified locally before restore.

Do not implement multi-host distribution until a real deployment has more than
one m80 host rebuilding the same template often enough for the rebuild cost to
matter.

## Candidate Mechanisms

The current fallback is still build-per-host. It is simple, local, and has the
fewest trust assumptions. The first multi-host mechanism should beat that in
measured operator value, not just in elegance.

Shared filesystem:

- Multiple hosts see the same template store path.
- This preserves local file semantics only if locking, ownership, and
  crash-recovery behavior are equivalent across hosts.
- Restore must still verify the manifest and fingerprint against local live
  inputs before using a body.
- This is operationally fragile unless the deployment already treats the shared
  filesystem as trusted infrastructure.

Content-addressed registry:

- Templates are exported as immutable bundles addressed by
  `TemplateFingerprint`.
- A host explicitly pulls a bundle into its local template store, verifies it,
  and only then can restore from the local copy.
- The registry is an upstream cache, not the live restore path.
- This is the most likely future direction if multi-host m80 becomes common.

Explicit push/pull:

- Operators build on one host and run `m80 template push` or
  `m80 template pull`.
- This keeps distribution visible in scripts and logs.
- It avoids background network fetches in the lease path.
- It needs the same verification and signing story as a registry.

## Trust And Provenance

The template fingerprint proves that a template body matches m80's typed input
tuple. It does not prove that the build pipeline was honest. If templates cross
a host boundary controlled by the same operator over a trusted channel, the
fingerprint plus local verification may be enough.

If templates cross an untrusted boundary, or if compliance requires provenance,
distribution depends on the image/template signing direction in
`docs/future-directions/image-template-signing.md`. Signing should cover the
template bundle manifest, snapshot body digests, pmem image digest set, guest
kernel digest, Firecracker version, and build provenance. Verification belongs
at import or pull time before the template enters the local store.

## Versioning

Remote lookup is by `TemplateFingerprint`, not by a friendly name. A host that
changes kernel, Firecracker, guest kernel, pmem layer set, post-init state, or
hook set computes a different fingerprint and must miss the old template.

Importing a remote template must:

1. parse the bundle manifest with `deny_unknown_fields`;
2. verify every body digest named by the manifest;
3. recompute the fingerprint from the manifest and compare it to the address;
4. compare the fingerprint to local live inputs before restore;
5. commit the body immutably to the local template store.

No compatibility shim should translate old fingerprints into new ones. A
mismatch is a cache miss or a typed import failure.

## Cache Locality

The host-local template store remains the authority for restore. The fill worker
may build locally or pull a verified bundle, but lease hand-back must not depend
on a network fetch. Ready slots should be backed by local snapshot files and
local image-store artifacts.

Image-store artifacts remain separate operator inputs. Template distribution
does not make template eviction responsible for deleting images, and it does
not bypass Shared pmem active-use markers.

## Non-Goals For v0.x

- No background template daemon that silently fetches remote templates.
- No direct restore from a remote filesystem or registry.
- No cross-host serving of old ready slots while a new fingerprint is building.
- No best-effort fallback from a failed remote import to a stale local template.
- No distribution format before signing/provenance requirements are known.

Until a multi-host scenario exists, keep the single-host template store and let
each host rebuild its own templates.
