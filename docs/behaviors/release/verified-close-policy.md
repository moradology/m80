# Release Verified-Close Policy

Release and quickstart proof work is not closed from green fixtures alone when
the bead depends on public latest behavior, hostless proof output, real-KVM
substrate, or uploaded release-proof artifacts. Those beads carry the
`requires-verified-close` label while they are open, and parent beads inherit
the label when one of their open descendants requires it.

`scripts/verify-release-tracker-policy.py` is the CI guard for this convention.
It reads `.beads/issues.jsonl` and fails when an open proof-shaped leaf or
inherited parent in a configured release epoch lacks `requires-verified-close`.
New proof-shaped leaves closed after this guard landed are checked too, so
closing first cannot bypass the label requirement. It also checks closed labeled
leaves: the close reason must include
`verified: <artifact-path> @ <commit-sha>`, the artifact path must be relative,
the artifact must exist in the current tree and in the cited commit, and the
artifact must contain the proof fields needed for quickstart evidence:
command, stdout/stderr or log path, exit status, resolved tag, and substrate.
CI checks out full history for this guard because close reasons may cite older
artifact commits.

The configured release epochs live in
`docs/behaviors/release/release-tracker-policy.json`. Before opening the next
release or quickstart epoch, add it there as an active epoch:

```json
{
  "id": "m80-next",
  "status": "active"
}
```

Retired epochs remain in the config only when they have `status: "retired"` and
a nonempty `reason`. The verifier fails closed on missing active epochs,
duplicate epoch ids, unknown statuses, or retired entries without reasons. For
local one-off checks, `scripts/verify-release-tracker-policy.py --epic <id>`
still lints a single epoch without using the config; CI uses the config-backed
path.

The policy intentionally keeps scaffolded work separate from verified close.
Unit tests, fake release servers, hostless fixtures, and validators may land and
remain useful before the final substrate proof exists. The bead stays open
until a committed artifact records the real proof named by the bead.

Relevant tests:

- `scripts/test-release-tracker-policy.py`
- CI `release script tests` runs both the fixture tests and the live tracker
  lint.
