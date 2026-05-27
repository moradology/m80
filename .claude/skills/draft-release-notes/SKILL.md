---
name: draft-release-notes
description: Suggest a Keep-a-Changelog section for a tag range. Outputs draft release-note text for human review before anything lands in CHANGELOG.md.
---

# Draft Release Notes

Suggest release-note text for m80. Do not edit files, commit, tag, publish, or
rewrite `CHANGELOG.md` unless the user separately asks for that.

The output is advisory. A human release operator must read, check, refine,
delete, merge, and approve the text before it becomes the committed
`CHANGELOG.md` section.

## Inputs

Accepted forms:

- `/draft-release-notes <previous-tag>..<target-tag>`
- `/draft-release-notes <previous-tag>..HEAD --tag <target-tag>`
- `/draft-release-notes --backfill <first-tag>..<latest-tag>`

If no range is supplied, use:

- previous tag: `git describe --tags --abbrev=0`
- target: `HEAD`
- target tag: ask the operator for the intended `vX.Y.Z`

## Evidence To Collect

Always collect:

```bash
git log --no-merges --format='%h %s%n%b' <previous-tag>..<target>
git diff --stat <previous-tag>..<target>
git diff --name-only <previous-tag>..<target>
```

Read the existing changelog context:

```bash
sed -n '1,220p' CHANGELOG.md
```

Use bead context only as advisory closure-time context:

```bash
br changelog --since-tag <previous-tag> --json
```

Per `docs/exploration/br-changelog-usability.md`, `br changelog` is based on
bead `closed_at`, not commit membership in the tag range. Empty output does not
mean no work shipped. Non-empty output may include beads closed after the
previous tag that are not part of the target release commit range. Cross-check
against `git log` before using any bead in a bullet.

For commits that mention bead ids, inspect the relevant bead:

```bash
br show <bead-id> --json
```

## Drafting Rules

- Start with `Draft suggestion — human review required`.
- Use the exact heading shape `## [vX.Y.Z] — YYYY-MM-DD`.
- Add a short 1-3 sentence release summary after the heading.
- Use Keep-a-Changelog subsections in this order: `Added`, `Changed`, `Fixed`,
  `Removed`.
- Omit empty subsections.
- Group by user-visible effect, not by commit count, author, or file path.
- Prefer present-tense, factual bullets.
- Do not invent benefits or user impact that is not supported by the evidence.
- Internal-only maintenance may be omitted or grouped into one short bullet if
  it matters to operators.
- If unsure, leave the bullet out and list it under `Consider also`.
- Do not include model provenance, prompt notes, or review process text inside
  the changelog section itself.

## Output Shape

Print this structure:

```markdown
Draft suggestion — human review required

## [vX.Y.Z] — YYYY-MM-DD

<summary paragraph>

### Added
- <bullet>

### Changed
- <bullet>

### Fixed
- <bullet>

### Removed
- <bullet>

Consider also:
- <commit or bead that needs human judgment>

Reviewer checklist:
- [ ] Checked each bullet against the commit range.
- [ ] Removed speculative, duplicated, or purely internal bullets.
- [ ] Verified user/operator-facing wording.
- [ ] Edited the final text into CHANGELOG.md manually.
```

Omit empty `Consider also` and empty subsection blocks.

## Backfill Mode

For `/draft-release-notes --backfill v0.2.0..v0.2.25`:

1. Enumerate stable tags in version order.
2. Draft one release section per adjacent range.
3. Stop after each section and ask the human operator to review/refine before
   moving to the next one.
4. Use existing `[Unreleased]` material as source text, but do not assume it is
   already grouped correctly.

Backfill output remains draft text. The human-reviewed `CHANGELOG.md` edit is
the only canonical release-note artifact.
