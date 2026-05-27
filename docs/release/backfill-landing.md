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

After the `--require-reviewed` gate passes, write the reviewed post-backfill
`[Unreleased]` body to a temporary file. This file contains only the body below
`## [Unreleased]`, not the heading itself.

Then apply the reviewed backfill:

```sh
python3 scripts/apply-backfill-review.py \
  --review docs/release/backfill-review.md \
  --changelog CHANGELOG.md \
  --post-backfill-unreleased /tmp/m80-reviewed-unreleased.md
```

If there is truly no post-backfill work, use `--empty-unreleased` instead of
`--post-backfill-unreleased`.

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
