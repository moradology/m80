# Backfill Landing Workflow

`docs/release/backfill-review.md` is a draft packet. It is not the changelog and
does not become release truth until a human release operator reviews and edits
the sections.

## Review

For each `v0.2.*` row:

1. Compare the draft section with `git log --no-merges <previous-tag>..<tag>`.
2. Keep only accurate, user/operator-relevant bullets.
3. Rewrite wording for clarity and merge duplicated bullets.
4. Change the coverage row status from `pending human review` to
   `human-reviewed by <name> on YYYY-MM-DD`.

The model suggests. The human reviewer decides what survives.

## Verify

Structural verification is allowed while sections are still pending:

```sh
python3 scripts/verify-backfill-review.py --review docs/release/backfill-review.md --check-git-ranges
```

The landing gate must require explicit human review:

```sh
python3 scripts/verify-backfill-review.py \
  --review docs/release/backfill-review.md \
  --check-git-ranges \
  --require-reviewed
```

## Land

After the `--require-reviewed` gate passes, copy the accepted sections into
`CHANGELOG.md` in reverse chronological order after `[Unreleased]`. Leave
`[Unreleased]` with only work that has not shipped.

Then prove extraction for every backfilled tag:

```sh
for tag in $(git tag --list 'v0.2.*' --sort=version:refname); do
  python3 scripts/extract-release-notes.py \
    --changelog CHANGELOG.md \
    --tag "$tag" \
    --out "/tmp/m80-release-notes-$tag.md"
done
```

Do not close `m80-6egdo.4` until the reviewed sections are in `CHANGELOG.md`,
`[Unreleased]` has been cleaned up, and the extraction loop succeeds.
