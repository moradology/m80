# GitHub Release Notes

m80 publishes GitHub Release bodies from committed `CHANGELOG.md` content.

For a new release tag, the release workflow extracts the body under the exact
header:

```text
## [vX.Y.Z] — YYYY-MM-DD
```

The extracted body is written to a temporary notes file and passed to:

```text
gh release create "$GITHUB_REF_NAME" --notes-file "$notes_file"
```

If `CHANGELOG.md` has no matching reviewed section, the workflow fails before
creating the GitHub Release. There is no one-line fallback for new releases.
Existing public releases remain read-only validation targets; the workflow does
not rewrite historical public release bodies.

The release notes section is human-reviewed source text. Agent-generated draft
bullets are suggestions only and are not consumed directly by automation.

Proof:

- `scripts/extract-release-notes.py` implements the exact extraction and
  fail-closed behavior.
- `scripts/test-extract-release-notes.py` covers matching sections,
  idempotence, missing-section failure, and malformed headers.
- `scripts/test-release-bundle.py` checks that
  `.github/workflows/release-artifacts.yml` invokes the extractor before
  `gh release create` and uses `--notes-file`.
- `scripts/verify-release-notes-proof.py` verifies the live GitHub Release body
  against the committed changelog section before writing the C7 proof artifact.
