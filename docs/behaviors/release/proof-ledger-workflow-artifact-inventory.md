# Proof Ledger Workflow Artifact Inventory

Release proof sidecars are explicit workflow evidence, not inferred files.
`m80-release-upload-manifest.json` schema version 2 has two separate workflow
views:

- `non_public_workflow_artifacts` names files that must never be public release
  assets;
- `workflow_artifact_inventory` digest-binds every proof sidecar that later
  evidence consumers must inspect.

The digest-bound proof sidecar inventory contains:

- `m80-release-proof-ledger.jsonl`;
- `m80-quickstart-proof-hostless.json`;
- `m80-quickstart-proof-hostless.verifier-result.json`;
- `m80-quickstart-stderr.txt`;
- `m80-quickstart-host-binaries.manifest.json`.

The upload manifest verifier rejects missing or stale sidecars before the build
artifact can be used as publish input. Public redownload verification still
checks the inventory shape and reasons but does not require workflow-only files
to be present in the public asset directory.

`m80-release-evidence.json` copies the same digest-bound sidecar inventory into
`workflow_only_artifacts` and verifies it against the downloaded workflow
artifact. Missing stderr, missing host-binaries manifest, stale verifier
result, stale proof JSON, extra inventory rows, and public/workflow asset
overlap fail closed before publish evidence is accepted.

Regression coverage:

- `scripts/test-release-bundle.py::test_release_upload_manifest_schema_derives_public_and_non_public_assets`
- `scripts/test-release-bundle.py::test_release_upload_manifest_rejects_missing_stderr_sidecar`
- `scripts/test-release-bundle.py::test_release_upload_manifest_rejects_missing_host_binaries_manifest_sidecar`
- `scripts/test-release-bundle.py::test_release_upload_manifest_rejects_stale_verifier_result_sidecar`
- `scripts/test-release-bundle.py::test_release_evidence_bundle_writes_schema_entrypoint`
- `scripts/test-release-bundle.py::test_release_evidence_bundle_rejects_missing_stderr_sidecar`
- `scripts/test-release-bundle.py::test_release_evidence_bundle_rejects_missing_host_binaries_manifest`
- `scripts/test-release-bundle.py::test_release_evidence_bundle_rejects_stale_verifier_result_digest`
- `scripts/test-release-bundle.py::test_release_evidence_bundle_rejects_public_workflow_artifact_confusion`
