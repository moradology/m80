# Changelog Discipline

Every pull request should either update `CHANGELOG.md` `[Unreleased]` or explain
why the change is skip-eligible.

Use the PR template checkbox:

- Update `[Unreleased]` with one or more human-reviewed bullets for user- or
  operator-visible changes.
- Choose changelog-skip only for purely internal tests, typo-only docs,
  dependency-only changes without behavior impact, or narrow audit cleanup.
  Include a justification.

CI runs `scripts/check-changelog-entry.py` on pull requests. If the PR omits
`CHANGELOG.md`, CI emits a GitHub Actions warning and still exits successfully.
The warning is advisory; the human reviewer enforces whether the skip reason is
credible.

`/draft-release-notes <base>..<head>` is available as optional drafting help.
The draft is not the changelog. The author or reviewer must read it, verify it
against the commit range, remove speculative or internal bullets, and commit
only reviewed text.
