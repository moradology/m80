# Release Evidence Bundle

`m80-release-evidence.json` is the stable machine-readable entrypoint for
release publish proof. It is schema-owned by
`scripts/release_evidence_bundle.py`.

The bundle records the concrete release tag, source commit, workflow run id,
m80 version, resolved install tag, required readiness lane ids, missing required
lane ids, and digest refs for the upload manifest, build handoff manifest,
publish decision receipt, and proof ledger. Top-level file refs use flat release
file names plus `sha256:<digest>` and `size_bytes`; public and workflow-only
artifact rows use raw lowercase sha256 digests to mirror the upload manifest.
None of these fields carry host-local paths.

The schema keeps three artifact classes distinct:

- `public_assets`: files expected on the GitHub Release, copied from
  `m80-release-upload-manifest.json`.
- `workflow_only_artifacts`: workflow artifacts that support release decisions
  but are not public release downloads, such as
  `m80-release-upload-manifest.json` and the hostless proof ledger.
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
redaction leaves may add stronger scanners, but schema version 1 already names
these forbidden categories.

The validator rejects missing required keys, unknown schema versions, duplicate
lane ids, malformed digests, stale file refs, public/workflow artifact overlap,
unaccounted required lanes, and proof entries whose artifact class points at the
wrong artifact set.
