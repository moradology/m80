# br changelog usability for release notes

Date: 2026-05-27

## Result

`br changelog` is useful as optional bead-closure context, but it is not a
reliable source for release notes by tag range in this repo.

The release-notes drafting skill should use `git log <prev>..<tag>` and the
human-reviewed `CHANGELOG.md` material as its canonical inputs. It may include
`br changelog` output only as advisory context when a date or tag happens to
cover bead closures relevant to the release.

## Evidence

Recent stable tags are close together:

```text
v0.2.23 e18971f41f26edc6088811e5531105fbfd3114b0 2026-05-27T09:37:53Z Bump workspace version to 0.2.23
v0.2.24 f44de951eb73fc805648113bb7e2aea9b63e6e19 2026-05-27T10:10:47Z Always publish flat jailer harden binary
v0.2.25 8e37e611764f83c2dff5c5cd3f9f07fb33df05c5 2026-05-27T11:44:46Z Prepare v0.2.25 release
```

The current adjacent stable range has no bead closures after the previous tag:

```text
$ br changelog --since-tag v0.2.24 --json
{
  "since": "v0.2.24",
  "total_closed": 0,
  "groups": []
}
```

The same is true for the broader recent tag:

```text
$ br changelog --since-tag v0.2.23 --json
{
  "since": "v0.2.23",
  "total_closed": 0,
  "groups": []
}
```

Direct date mode does return closed beads when the timestamp encloses actual
closure events:

```text
$ br changelog --since 2026-05-27T09:25:00Z --json
{
  "since": "2026-05-27T09:25:00Z",
  "total_closed": 7,
  "groups": [
    {
      "issue_type": "epic",
      "label": "Epics",
      "issues": [
        {
          "id": "m80-92eor",
          "title": "Phase 2 investigation: get upstream Firecracker jailer to apply final-exec-site hardening",
          "priority": "P2",
          "closed_at": "2026-05-27T09:32:30.507594+00:00"
        }
      ]
    },
    {
      "issue_type": "task",
      "label": "Tasks",
      "issues": [
        {
          "id": "m80-92eor.6",
          "title": "ADR closing Phase 2 investigation: go-forward plan",
          "priority": "P2",
          "closed_at": "2026-05-27T09:32:29.822787974+00:00"
        }
      ]
    }
  ]
}
```

The JSON shape is:

```text
{
  "since": "<input range marker>",
  "until": "<query timestamp>",
  "total_closed": <number>,
  "groups": [
    {
      "issue_type": "epic|task|bug|feature",
      "label": "<plural label>",
      "issues": [
        {
          "id": "<bead id>",
          "title": "<bead title>",
          "priority": "P<n>",
          "closed_at": "<RFC3339 timestamp>"
        }
      ]
    }
  ]
}
```

## Root cause

`br changelog` is closure-time based. `--since-tag <tag>` resolves the tag to a
timestamp, then reports beads whose `closed_at` is after that timestamp. It does
not inspect commit membership in `<previous-tag>..<tag>`, and it does not map
beads to commits.

That means it can return zero for a release range that has real commits but no
beads closed after the previous tag. For example:

```text
$ git log --oneline v0.2.24..v0.2.25
8e37e611 Prepare v0.2.25 release
77eb2c7b Mark v0.2.24 public installer fresh
```

`br changelog --since-tag v0.2.24` still returns `total_closed: 0` because no
beads closed after the `v0.2.24` tag timestamp.

## Decision for m80-6egdo

`m80-6egdo.2` should not depend on `br changelog` for correctness.

The drafting skill should:

- Always collect `git log --no-merges --format='%h %s%n%b' <prev>..<tag>`.
- Always read the relevant `CHANGELOG.md` material.
- Optionally run `br changelog --since-tag <prev> --json` and include any
  non-empty output as context, clearly labeled as closure-time context.
- Treat empty `br changelog` output as normal, not as proof that no work shipped.
- Produce only draft suggestions; a human reviewer decides what lands in
  `CHANGELOG.md`.

No upstream `br` issue is needed for this epic. The command is behaving like a
closed-bead changelog, not a release-tag diff generator.
