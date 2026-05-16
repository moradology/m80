# Image Store Layout

Phase B ships a simple content-addressed image store for sibling erofs and ext4
artifacts. The planned layout is:

```text
<store-root>/<digest[0..2]>/<digest>/image.erofs
<store-root>/<digest[0..2]>/<digest>/image.ext4
<store-root>/<digest[0..2]>/<digest>/metadata.json
```

The store ingests pre-built artifacts, verifies their digest, records a small
`deny_unknown_fields` metadata sidecar, and resolves by digest. It does not own
Nix, mkosi, distro tooling, or operator CI image-build policy.

This layout should stay boring until a concrete consumer forces more shape. Do
not expand without a concrete consumer. Named tags, retention classes,
multi-host distribution, richer metadata indexes, and separate erofs/ext4 store
roots are future options, not Phase B requirements.

Conditions that could justify evolving the layout:

- garbage collection needs refcount-aware retention across shared pmem layers;
- operators need named promotion channels in addition to immutable digests;
- template fingerprints need a store-level index for fast invalidation;
- multi-host distribution needs signatures or provenance attached to artifacts.

Until one of those lands as a real bead, the digest path is the contract.
