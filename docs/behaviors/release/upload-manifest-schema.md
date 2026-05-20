# Release Upload Manifest Schema

The tag release workflow produces `m80-release-upload-manifest.json` as the
workflow-local source of truth for the publish job. The manifest is not a
public GitHub Release asset. It separates public release assets from files that
exist only inside the Actions artifact so upload and re-download lists cannot
drift from the signed release-integrity material.

Schema version 1 contains:

- `schema_version`: `1`;
- `release_tag`: the concrete tag being published;
- `public_assets`: rows with `name`, `kind`, `sha256`, `size_bytes`, and
  `integrity_subject`;
- `non_public_workflow_artifacts`: rows with `name` and `reason`.

Public rows are derived from `m80-release-integrity.json`. Every integrity
subject becomes a public asset with `integrity_subject: true`, and its name,
kind, size, and digest must match the local dist file. The post-predicate
public proof assets are limited to:

- `m80-release-integrity.attestation.jsonl` with kind
  `github-artifact-attestation-bundle`;
- `m80-release-attestation.json` with kind
  `release-attestation-metadata`.

The integrity predicate itself is also public as
`m80-release-integrity.json` with kind `release-integrity-predicate` and
`integrity_subject: false`.

Workflow-only artifacts are listed explicitly with reasons. Version 1 names
`m80-release-upload-manifest.json` as the workflow-only source of upload and
redownload truth, and `m80-quickstart-proof-hostless.json` as hostless workflow
evidence whose public proof is the signed release-integrity predicate plus
attestation material.

`scripts/release_upload_manifest.py --write` writes the manifest and then
verifies it. Verification fails closed on duplicate names, path traversal,
missing public files, stale `sha256`, stale `size_bytes`, unknown top-level
fields, public asset set drift, post-predicate proof asset drift, and
non-public workflow artifact drift.

Relevant tests:

- `scripts/test-release-bundle.py::test_release_upload_manifest_schema_derives_public_and_non_public_assets`
- `scripts/test-release-bundle.py::test_release_upload_manifest_rejects_duplicate_public_asset_name`
- `scripts/test-release-bundle.py::test_release_upload_manifest_rejects_path_traversal_name`
- `scripts/test-release-bundle.py::test_release_upload_manifest_rejects_missing_public_file`
- `scripts/test-release-bundle.py::test_release_upload_manifest_rejects_stale_public_digest`
- `scripts/test-release-bundle.py::test_release_upload_manifest_rejects_stale_public_size`
- `scripts/test-release-bundle.py::test_release_upload_manifest_rejects_unknown_top_level_field`
- `scripts/test-release-bundle.py::test_release_upload_manifest_rejects_undocumented_public_asset`
- `scripts/test-release-bundle.py::test_release_upload_manifest_rejects_undocumented_non_public_artifact`
