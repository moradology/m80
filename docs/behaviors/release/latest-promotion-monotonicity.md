# Latest Promotion Monotonicity

Behavior capture for bead `m80-o3uh9.13.2.2`.

## Contract

The protected release publish job must not silently move GitHub `latest` to an
older stable release while a newer stable public release exists.

Before `gh release edit --latest`, the workflow captures the current public
release list and runs `scripts/release_latest_promotion.py`. The decision
receipt `m80-latest-promotion-decision.json` records:

- the target release tag;
- the highest public non-draft, non-prerelease stable tag observed;
- the publish decision receipt digest;
- the proof ledger digest;
- the remote asset inventory digest;
- the rollback receipt digest when one was used;
- `approved`, `rollback_approved`, or `refused`;
- a human reason and remediation for refused decisions.

Latest promotion also validates `m80-release-remote-assets.json` against
`m80-release-upload-manifest.json` before approving. The remote inventory must
be for the target release tag, must not have duplicate asset names or ids, must
not omit or add public assets, and each row's kind, size, and SHA256 digest must
match the upload manifest. Missing, stale, incomplete, duplicate, or mismatched
remote state is refused before `gh release edit --latest`.

Normal promotion is approved only when the target tag is greater than or equal
to the highest public stable tag. Drafts, prereleases, and non-stable tag names
do not define the highest stable tag.

An older target requires a checked rollback receipt at
`docs/operations/release-latest-rollback-receipt.json`. The receipt must be a
schema-versioned `m80_release_latest_rollback_receipt` with `decision:
approved`, the target release tag, the highest stable public tag being
overridden, a reason, actor, publish decision digest, proof ledger digest, and a
generation timestamp. Stale or mismatched receipts fail before latest moves.

## Verification

`scripts/test-release-latest-promotion.py` covers highest-stable approval,
lower-tag refusal, explicit rollback approval, draft/prerelease exclusion,
stale rollback receipt digest refusal, missing remote inventory, stale remote
inventory, extra remote assets, and duplicate remote asset names.

`scripts/test-release-bundle.py` checks that the release workflow runs the
latest promotion decision before `gh release edit --latest` and uploads the
decision receipt as durable evidence.
