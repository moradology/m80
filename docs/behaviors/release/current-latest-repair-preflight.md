# Current Latest Repair Preflight

`m80-o3uh9.21.7.1.1` adds
`scripts/current_latest_repair_preflight.py`, the release-side stop sign for
repairing a broken public latest release. It runs before release packaging or
upload work in `.github/workflows/release-artifacts.yml`, and it writes a JSON
artifact even when the repair candidate is rejected.

The preflight accepts only a candidate whose release tag is the exact
`v<workspace.package.version>` from `Cargo.toml`, whose source commit is the
same commit named by the tag when the tag already exists, whose worktree is
clean, and whose stable tag is newer than the current public latest tag. This
prevents a current checkout from backfilling assets into an older release such
as the `v0.2.6` latest state that did not carry `install.sh`.

Current-latest metadata is required. If GitHub latest metadata is unavailable,
empty, or names a non-stable tag, the repair preflight fails closed before
packaging. A release that cannot prove what it is superseding must stop rather
than publish installer assets from an unknown release state.

The artifact schema is `1` and records `target_tag`, `source_commit`,
`workspace_package_version`, `expected_release_tag`, `dirty_tree`,
`existing_latest`, `tag`, `release_order`, and
`supersedes_missing_installer_latest_state`. Rejected candidates include
diagnostics with the mismatched field, expected value, observed value, and the
safe repair action: cut a matching stable tag, rerun the release workflow, or
stop for manual release-state repair.

Fixture coverage pins the clean new stable-tag path, dirty-tree rejection,
stale existing tag rejection, tag/version mismatch rejection, and attempted
old-release backfill rejection.
