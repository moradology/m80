# Release Publish Decision Receipt

The tag publish job must write and validate
`m80-release-publish-decision.json` before any `gh release upload` or latest
promotion command can run. The receipt is the publish-authority handoff: it
records which already-verified inputs justified moving public release state.

The receipt uses `schema_version: 1` and `kind:
"m80_release_publish_decision"`. It records:

- `release_tag`, `commit_sha`, `workflow_run_id`, `workflow_run_attempt`,
  `actor`, `repository`, and `github_ref`.
- `environment_approval_id` when GitHub exposes one to the job.
- `artifact_manifest_digest`, bound to
  `m80-release-upload-manifest.json`.
- `proof_ledger_digest`, bound to the current proof-ledger input. Until the
  durable proof ledger lands, the tag workflow uses the hostless quickstart
  proof artifact as that input.
- The public asset list copied from the upload manifest: name, kind, sha256,
  size, and whether it was covered by release-integrity material.

`scripts/release_publish_receipt.py --write` writes the receipt and then
validates it. A non-writing invocation validates an existing receipt against
the current dist directory, tag, commit, actor, repository, ref, upload
manifest, proof input, and public asset bytes.

The verifier fails closed for a missing receipt, stale upload-manifest digest,
stale proof-ledger digest, wrong actor, wrong repository, wrong ref, wrong tag,
wrong commit, stale public asset hash or size, and malformed receipt fields.
The publish job runs the verifier before upload. A later failed publish preserves
the attempted receipt in the failed publish diagnostics artifact; successful
publishes upload the receipt as a durable workflow artifact.

This receipt is not a signature and not a substitute for the underlying release
integrity material. It is the auditable decision record that says the publish
job had those verified inputs in hand before mutating GitHub release state.
