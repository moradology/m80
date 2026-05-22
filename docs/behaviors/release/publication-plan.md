# Release Publication Plan

The tag publish job writes `m80-release-publication-plan.json` after the publish
decision receipt and before any GitHub release mutation. The plan is generated
by `scripts/release_publication_plan.py` from the upload manifest and the
current GitHub release metadata for the tag.

The allowed actions are:

- `create_draft_upload_publish`: the GitHub release is absent. The workflow
  creates a draft release with `--verify-tag`, uploads the manifest-selected
  public assets without `--clobber`, re-downloads and validates the uploaded
  draft assets, and publishes it as latest only after that pre-promotion
  validation passes.
- `validate_existing_public_release`: a non-draft, non-prerelease public
  release already exposes the complete asset name and size set. The workflow
  skips upload and validates the remote bytes by re-downloading them.
- `fail_manual_recovery_required`: the release exists as a draft, prerelease,
  incomplete public release, or metadata mismatch. The job fails before upload
  and records the manual recovery command or instruction.

This keeps reruns fail-closed. A successful rerun over identical public bytes is
a validation pass, not a clobber. Missing or size-mismatched public assets fail
before upload; same-size byte drift fails during the remote asset inventory
redownload because `m80-release-remote-assets.json` is computed from GitHub's
served bytes.

Draft recovery is intentionally manual. If a prior publish attempt leaves a
draft release behind, delete the draft release without deleting the tag:

```sh
gh release delete <version> --yes
```

Then rerun the protected tag workflow for the same tag.

Coverage:

- `scripts/test-release-publication-plan.py` covers absent, existing public,
  draft, missing-asset, and size-mismatch plans.
- `scripts/test-release-bundle.py` checks that the release workflow uses the
  publication plan, creates drafts before upload, validates uploaded draft
  assets before latest promotion, and never uses `gh release upload --clobber`.
- `crates/m80-cli/tests/release/publication_plan.rs` pins this behavior doc and
  workflow wiring from the crate-level release test suite.
