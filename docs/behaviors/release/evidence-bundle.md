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
mirror the upload manifest. None of these fields carry host-local paths.

The schema keeps three artifact classes distinct:

- `public_assets`: files expected on the GitHub Release, copied from
  `m80-release-upload-manifest.json`.
- `workflow_only_artifacts`: workflow artifacts that support release decisions
  but are not public release downloads, such as
  `m80-release-upload-manifest.json`, `m80-release-proof-ledger.jsonl`, and
  `m80-quickstart-proof-hostless.json`.
- `proofs`: lane-specific evidence entries that name their substrate and point
  to either a public or workflow-only artifact.

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
redaction leaves may add stronger scanners, but schema version 2 already names
these forbidden categories.

The validator rejects missing required keys, unknown schema versions, duplicate
lane ids, malformed digests, stale file refs, public/workflow artifact overlap,
unaccounted required lanes, and proof entries whose artifact class points at the
wrong artifact set. It also rejects proof rows that point at the proof ledger,
swapped or duplicated proof file refs, missing proof JSON, and stale proof JSON
or proof-ledger digests.
