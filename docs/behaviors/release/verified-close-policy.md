# Release Verified-Close Policy

Release and quickstart proof work is not closed from green fixtures alone when
the bead depends on public latest behavior, hostless proof output, real-KVM
substrate, or uploaded release-proof artifacts. Those beads carry the
`requires-verified-close` label while they are open, and parent beads inherit
the label when one of their open descendants requires it.

`scripts/verify-release-tracker-policy.py` is the CI guard for this convention.
It reads `.beads/issues.jsonl` and fails when an open `m80-o3uh9` proof-shaped
leaf or inherited parent lacks `requires-verified-close`. New proof-shaped
leaves closed after this guard landed are checked too, so closing first cannot
bypass the label requirement. It also checks closed labeled leaves: the close
reason must include
`verified: <artifact-path> @ <commit-sha>`, the artifact path must be relative,
the artifact must exist in the current tree and in the cited commit, and the
artifact must contain the proof fields needed for quickstart evidence:
command, stdout/stderr or log path, exit status, resolved tag, and substrate.
CI checks out full history for this guard because close reasons may cite older
artifact commits.

The policy intentionally keeps scaffolded work separate from verified close.
Unit tests, fake release servers, hostless fixtures, and validators may land and
remain useful before the final substrate proof exists. The bead stays open
until a committed artifact records the real proof named by the bead.

Relevant tests:

- `scripts/test-release-tracker-policy.py`
- CI `release script tests` runs both the fixture tests and the live tracker
  lint.
