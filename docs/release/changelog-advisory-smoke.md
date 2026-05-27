# Changelog Advisory Smoke

Date: 2026-05-27

This smoke simulates the pull-request advisory path with a temporary git repo:
one source file changed, no `CHANGELOG.md` change.

Command shape:

```text
python3 scripts/check-changelog-entry.py \
  --event-name pull_request \
  --base <base-sha> \
  --head HEAD \
  --repo <fixture-repo>
```

Output:

```text
::warning title=CHANGELOG.md not changed::This pull request does not modify CHANGELOG.md. Update [Unreleased] with human-reviewed release-note text, or mark the PR template changelog-skip checkbox with a justification.
```

The command exits 0. The warning is advisory and points reviewers back to the
PR template instead of blocking legitimate changelog-skip pull requests.
