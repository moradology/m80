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
mirror the upload manifest. Schema version 4 records `host_binaries` and
`release_identity` summary rows for quickstart proof lanes. These rows are
normalized evidence inputs, not log scrapes: host-binaries rows are built from
the quickstart proof's relative `host_binaries.manifest_path`, the upload
manifest's digest inventory for that manifest, the proof/manifest Firecracker
and jailer versions, and an install-root classification. Release-identity rows
are built from the proof's requested/resolved tag, m80 version/release tag, and
the bundle metadata file's release tag, m80 version, manifest schema version,
and guest protocol version. None of these fields carry host-local paths.

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

Hostless proof never satisfies real-KVM proof. A hostless quickstart fixture is
recorded with `lane_id=hostless-quickstart`, `substrate=hostless`, and
`artifact_class=workflow-only`. A real-KVM quickstart proof must use
`lane_id=real-kvm-quickstart` and `substrate=real-kvm`. Required lanes that do
not yet have a proof entry must appear in `missing_required_lane_ids`, so the
bundle can distinguish "required and absent" from "satisfied by the wrong
substrate."

The redaction rule is part of the schema contract: evidence bundles may contain
flat release file names, sizes, digests, release tags, commit SHA, workflow run
ids, m80 version, lane ids, proof kinds, and substrate names. They must not
contain host absolute paths, secrets, tokens, or environment dumps. Follow-up
redaction leaves may add stronger scanners, but schema version 4 names these
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
