# Draft Release Notes

m80 uses a project-level Claude Code skill for release-note drafting:

```text
/draft-release-notes <previous-tag>..<target-tag>
```

Example:

```text
/draft-release-notes v0.2.24..v0.2.25
```

The same instructions are plain Markdown at:

```text
.claude/skills/draft-release-notes/SKILL.md
```

Codex or another agent can read that file and follow it manually.

## What It Does

The skill gathers:

- commit subjects and bodies from `git log <previous-tag>..<target-tag>`
- file-level scope from `git diff --stat` and `git diff --name-only`
- existing `CHANGELOG.md` context
- optional bead closure context from `br changelog --since-tag <previous-tag>`

Then it prints a Keep-a-Changelog-shaped draft:

```markdown
Draft suggestion — human review required

## [vX.Y.Z] — YYYY-MM-DD

Short release summary.

### Changed
- Release-note bullet.

Reviewer checklist:
- [ ] Checked each bullet against the commit range.
- [ ] Removed speculative, duplicated, or purely internal bullets.
- [ ] Verified user/operator-facing wording.
- [ ] Edited the final text into CHANGELOG.md manually.
```

## Human Review Is Required

The skill does not edit `CHANGELOG.md`, commit, tag, publish, or call an API.

The release operator owns the final text. Treat the draft as suggestions:

- verify each bullet against the commit range
- discard model text that is speculative, duplicated, too internal, or not
  user-visible
- merge related bullets
- rewrite wording to match the release surface
- commit only the reviewed `CHANGELOG.md` section

Release automation consumes committed `CHANGELOG.md` only. It never consumes
raw model output.

## br changelog Caveat

`br changelog` is closure-time based, not commit-range based. A non-empty result
can include beads closed after the previous tag that are not part of the target
release. Empty output does not prove that no commits shipped.

Use `docs/exploration/br-changelog-usability.md` as the source for this
decision. In the draft, `git log <previous-tag>..<target-tag>` is canonical;
`br changelog` is optional context.

## Backfill

For historical release sections:

```text
/draft-release-notes --backfill v0.2.0..v0.2.25
```

Backfill mode drafts one adjacent tag range at a time and pauses for human
review between sections. The operator edits the reviewed sections into
`CHANGELOG.md` in reverse-chronological order.
