# Release Evidence Bundle

`m80-release-evidence.json` is the stable machine-readable entrypoint for
release publish proof. It is schema-owned by
`scripts/release_evidence_bundle.py`.

The bundle records the concrete release tag, source commit, workflow run id,
m80 version, resolved install tag, required readiness lane ids, missing required
lane ids, and digest refs for the upload manifest, build handoff manifest,
publish decision receipt, and proof ledger. Schema version 2 keeps the proof
ledger and each lane proof JSON as distinct file refs: the top-level
`proof_ledger` points at `m80-release-proof-ledger.jsonl`, while the hostless
quickstart proof row points at `m80-quickstart-proof-hostless.json`. Top-level
file refs use flat release file names plus `sha256:<digest>` and `size_bytes`;
public and workflow-only artifact rows use raw lowercase sha256 digests to
mirror the upload manifest. Schema version 5 records `host_binaries`,
`release_identity`, and `substrate_policy` summary rows for quickstart proof lanes. These rows are
normalized evidence inputs, not log scrapes: host-binaries rows are built from
the quickstart proof's relative `host_binaries.manifest_path`, the upload
manifest's digest inventory for that manifest, the proof/manifest Firecracker
and jailer versions, and an install-root classification. Release-identity rows
are built from the proof's requested/resolved tag, m80 version/release tag, and
the bundle metadata file's release tag, m80 version, manifest schema version,
and guest protocol version. Substrate-policy rows are built from the proof
payload and `docs/behaviors/release/release-readiness-lanes.json`; they name
the lane, logical proof kind, observed proof/substrate kind, required substrate
class, fixture status, and whether that proof can satisfy publish or latest
promotion for its own lane. None of these fields carry host-local paths.
Schema version 6 adds `receipt_refs`: digest-bound refs for publish/latest-time
receipts that may arrive after build and proof collection. Each row names a
known receipt id, artifact class, file ref, schema version, kind, release tag,
commit SHA, and workflow run id. The verifier recomputes the receipt file
digest and size from the artifact root, rejects unknown receipt ids, and checks
the receipt payload's schema, kind, release tag, commit SHA, and workflow run id
when that payload owns those fields.

The schema keeps three artifact classes distinct:

- `public_assets`: files expected on the GitHub Release, copied from
  `m80-release-upload-manifest.json`.
- `workflow_only_artifacts`: digest-bound workflow artifacts that support
  release decisions but are not public release downloads. They are derived
  from the upload manifest's `workflow_artifact_inventory` and include
  `m80-release-proof-ledger.jsonl`,
  `m80-quickstart-proof-hostless.json`,
  `m80-quickstart-proof-hostless.verifier-result.json`,
  `m80-quickstart-stderr.txt`, and
  `m80-quickstart-host-binaries.manifest.json`.
- `proofs`: lane-specific evidence entries that name their substrate and point
  to either a public or workflow-only artifact.
- `host_binaries`: lane-specific summaries for the host-binaries manifest used
  by each quickstart proof. The summary names the lane, substrate,
  workflow-only manifest file ref, Firecracker version, jailer version, and
  install-root classification (`hostless-fixture`, `default-install-root`, or
  `override-install-root`). It deliberately omits raw install-root paths.
- `release_identity`: lane-specific release identity summaries for the
  quickstart proof and bundle metadata. The summary names the requested tag
  (`latest` or a concrete tag), resolved install tag, m80 version, m80
  release tag, bundle release tag, bundle m80 version, manifest schema version,
  guest protocol version, and digest-bound bundle metadata file ref. These are
  release identity inputs, not informational log text.
- `substrate_policy`: lane-specific substrate summaries from the readiness
  config and proof payload. Hostless fixture proof may satisfy the configured
  hostless publish lane but may not satisfy latest promotion or the
  `real-kvm-quickstart` lane. The real-KVM lane must carry an observed
  `real-kvm` substrate from a real-KVM proof file.
- `receipt_refs`: publish/latest-time receipts that are optional because they
  appear at different points in the workflow. The evidence bundle always binds
  the publish decision receipt when present as a top-level mandatory ref and as
  a typed receipt row. It also binds the token-authority receipt, readiness
  decision, remote asset inventory, and public-access receipt whenever those
  files are present in the artifact root.

Hostless proof never satisfies real-KVM proof. A hostless quickstart fixture is
recorded with `lane_id=hostless-quickstart`, `substrate=hostless`, and
`artifact_class=workflow-only`. A real-KVM quickstart proof must use
`lane_id=real-kvm-quickstart` and `substrate=real-kvm`. Required lanes that do
not yet have a proof entry must appear in `missing_required_lane_ids`, so the
bundle can distinguish "required and absent" from "satisfied by the wrong
substrate."

Build/proof-time refs are mandatory because publish cannot even evaluate the
release without them:

- `upload_manifest`
- `build_handoff`
- `publish_decision_receipt`
- `proof_ledger`
- quickstart `proofs[*].file`
- proof-derived `host_binaries`, `release_identity`, and `substrate_policy`
  rows

Publish/latest-time receipt refs are optional at write time but strict once
present:

- `receipt_refs[publish-decision]` ->
  `m80-release-publish-decision.json`
- `receipt_refs[token-authority]` ->
  `m80-release-token-authority.json`
- `receipt_refs[readiness-decision]` ->
  `m80-release-readiness-decision.json`
- `receipt_refs[remote-asset-inventory]` ->
  `m80-release-remote-assets.json`
- `receipt_refs[public-access]` ->
  `release-readiness-public-access.json`

Optional does not mean best-effort. If a row is present and the file is missing,
stale, has the wrong schema or kind, or disagrees with the bundle release tag,
commit SHA, or workflow run id, verification fails closed and publish/latest
state must not move.

The redaction rule is part of the schema contract: evidence bundles may contain
flat release file names, sizes, digests, release tags, commit SHA, workflow run
ids, m80 version, lane ids, proof kinds, and substrate names. They must not
contain host absolute paths, secrets, tokens, or environment dumps. Follow-up
redaction leaves may add stronger scanners, but schema version 6 names these
forbidden categories and validates that generated bundle strings do not leak
common absolute host path prefixes.

The validator rejects missing required keys, unknown schema versions, duplicate
lane ids, malformed digests, stale file refs, missing or stale workflow-only
proof sidecars, public/workflow artifact overlap, unaccounted required lanes,
and proof entries whose artifact class points at the wrong artifact set. It
also rejects proof rows that point at the proof ledger, swapped or duplicated
proof file refs, missing proof JSON, and stale proof JSON or proof-ledger
digests. Host-binaries collection fails closed when a proof omits its manifest
path, points at an absolute or escaping manifest path, omits a Firecracker or
jailer version, disagrees with the manifest version fields, or when the manifest
file ref no longer matches the upload manifest's workflow artifact inventory.
Release-identity collection fails closed when latest remains unresolved, the
resolved install tag is not concrete, the m80 version/release tag disagrees with
the evidence command inputs, the proof bundle tag/version disagrees, or the
bundle metadata's release tag, m80 version, manifest schema version, or guest
protocol version no longer matches the proof fields.
Substrate-policy collection fails closed when a hostless proof is supplied for
the real-KVM lane, a real-KVM proof is supplied for the hostless lane, a
freshness proof is substituted for a release publish proof, or the observed
proof substrate is not allowed by the readiness config. Diagnostics name the
lane id, required substrate, observed substrate or proof kind, proof file, and
config path.
Receipt-ref verification fails closed when a receipt row points at stale bytes,
the receipt file is missing, the row names an unknown receipt id, the row's
schema/kind/tag/commit/workflow context disagrees with the bundle, or the
payload's schema, kind, release tag, commit SHA, or workflow run id disagrees
with the bundle context.

Every evidence bundle schema version must verify these core refs before publish
can move mutable release state:

- `upload_manifest` -> `m80-release-upload-manifest.json`
- `build_handoff` -> `m80-release-build.json`
- `publish_decision_receipt` -> `m80-release-publish-decision.json`
- `proof_ledger` -> `m80-release-proof-ledger.jsonl`
- `proofs[hostless-quickstart].file` ->
  `m80-quickstart-proof-hostless.json`
- the bundle file itself must be read from the top-level dist artifact root as
  `m80-release-evidence.json`

The verifier recomputes each core file ref from bytes under the dist artifact
root and fails closed when the referenced file is missing, the digest or size is
stale, the expected path escapes the artifact root, the proof file name is
duplicated, or the proof artifact class points at the wrong artifact set. Core
ref diagnostics include the field path, expected and actual name, digest, size,
and a repair command. A byte-for-byte digest of `m80-release-evidence.json` is
owned by the external publish receipt/evidence entrypoint rather than embedded
inside the same JSON file.
