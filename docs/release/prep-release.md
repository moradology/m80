# prep-release.sh

`scripts/prep-release.sh` performs the mechanical release-prep step after
release notes have already been drafted, reviewed, and edited into
`CHANGELOG.md` `[Unreleased]`.

Ordering:

1. Optional: run `/draft-release-notes <previous-tag>..<target-tag>` for a
   draft suggestion.
2. Required: human reviewer checks, refines, removes, merges, and commits only
   accurate text into `CHANGELOG.md` `[Unreleased]`.
3. Run prep-release:

   ```sh
   scripts/prep-release.sh --version vX.Y.Z
   ```

The script does not call the drafting skill or any LLM/API. It only promotes
reviewed changelog text and bumps the workspace version.

## Interface

```sh
scripts/prep-release.sh --version vX.Y.Z [--date YYYY-MM-DD] [--dry-run]
```

Defaults:

- `--version`: required
- `--date`: current UTC date
- `--dry-run`: print a diff preview and write nothing

## Behavior

The script refuses to continue if:

- the git tree is dirty
- the target tag already exists
- `CHANGELOG.md` lacks `## [Unreleased]`
- `[Unreleased]` is empty

On success it:

- rewrites `CHANGELOG.md` so `[Unreleased]` is fresh and empty
- inserts `## [vX.Y.Z] — YYYY-MM-DD` below it with the reviewed prior
  `[Unreleased]` body
- updates `[workspace.package] version` in `Cargo.toml`
- runs `cargo check --workspace`
- prints the exact manual next steps for commit, tag, and push

The script does not commit, tag, push, or publish.

## Exit Codes

- `0`: success
- `2`: invalid or missing arguments
- `3`: dirty git tree
- `4`: target tag already exists
- `5`: structural error in `CHANGELOG.md` or `Cargo.toml`
- `6`: empty `[Unreleased]`; draft/review release notes first

## Dry-Run Example

```sh
scripts/prep-release.sh --version v99.99.99 --date 2026-05-27 --dry-run
```

The command prints a `CHANGELOG.md` diff and a `Cargo.toml` diff, then exits
without writing either file.
