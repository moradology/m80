# Image And Template Signing

Bead: `m80-q420k.8.11`

Content addressing proves that stored bytes still match a digest. It does not
prove that the bytes came from a trusted build. A compromised image builder can
produce malicious bytes and a matching sha256 digest. Signing and provenance
become relevant only when m80 must trust artifacts built outside the local
operator boundary.

No signing implementation is required for v0.x. Add it only when the deployment
threat model includes cross-tenant use, untrusted operators, remote template
distribution, or supply-chain compliance requirements.

## What Would Be Signed

Image artifacts:

- artifact digest and kind (`erofs` or `ext4`);
- build recipe identity, source revision, and build environment;
- erofs compatibility facts used by `m80-image-store` import validation;
- intended role, such as base rootfs or pmem layer;
- optional operator metadata such as promotion channel or release name.

Snapshot templates:

- `TemplateFingerprint`;
- template manifest digest;
- `vm.snap` and `mem.snap` digests;
- Firecracker version and guest kernel digest;
- pmem image digest set and stable jail-visible layout;
- post-init state digest and hook-set digest;
- build host identity if the deployment considers host provenance relevant.

The signature target should be a manifest over these typed fields, not a
signature over ad hoc logs.

## Verification Boundary

Verification belongs at import or pull time, before bytes enter the local
content-addressed store:

- `ImageStore::import_existing` can verify image signatures alongside digest
  and erofs feature validation.
- A future template pull/import path can verify a template bundle before
  committing it under `TemplateStore`.
- `m80 image verify` and `m80 template verify` can re-run digest and signature
  checks for operator audits.

Do not reverify signatures on every VM launch in the hot path unless a measured
threat model requires it. Normal launch should trust an already admitted local
store artifact, just as it already trusts the digest and metadata written at
import. If strict signing mode is enabled, unsigned artifacts fail closed at
import; there should be no best-effort unsigned fallback.

## Sigstore And In-Toto Shape

Sigstore/cosign is a plausible signature transport for immutable image and
template bundle manifests. In-toto attestations are a plausible provenance
format for recording builder identity, source revision, inputs, and build steps.
The exact choice should be made when a real deployment names its trust roots and
compliance needs.

The likely integration is:

1. external build pipeline emits an image or template bundle;
2. pipeline emits a signed manifest and provenance attestation;
3. m80 import verifies the signature chain and digest bindings;
4. m80 stores the verified bytes and records signature/provenance metadata in
   its local store metadata;
5. later `verify` commands can audit the same bindings without rebuilding.

For airgapped deployments, the same m80 policy should also allow configured
offline public keys or an organization trust root. The policy should be explicit
in config; do not infer trust from path names, git remotes, or digest prefixes.

## Key Management

Possible trust roots:

- per-operator keys for small local deployments;
- organization-level roots for centrally built artifacts;
- keyless CI identities when the deployment already relies on that identity
  provider;
- offline keys for airgapped or regulated environments.

The store should record which policy admitted an artifact. A later stricter
policy should not silently grandfather old unsigned artifacts; it should report
them as policy failures until the operator reimports or explicitly keeps them
under a documented legacy policy.

## Relationship To Multi-Host Distribution

`docs/future-directions/multi-host-template-distribution.md` assumes remote
template bundles are verified before entering a local store. If the remote
boundary is trusted because all hosts are under one operator and distribution
uses a trusted channel, signatures may be unnecessary. If the registry,
filesystem, or builder is outside that boundary, signing becomes a prerequisite
for remote template reuse.

## Non-Goals For v0.x

- No signature fields in current image-store or template-store schemas.
- No automatic network calls to transparency logs during launch.
- No soft "warn but import anyway" mode for strict-signing deployments.
- No signing requirement for same-host, same-operator development.
- No provenance parser until a deployment names the exact provenance format it
  needs.

Until that threat model exists, immutable digests, typed metadata, fail-closed
template fingerprints, and local operator control remain the v0.x contract.
