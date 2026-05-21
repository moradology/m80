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
`docs/behaviors/release/release-tracker-policy.json`. CI runs:

```sh
python3 scripts/verify-release-tracker-policy.py \
  --policy-config docs/behaviors/release/release-tracker-policy.json
```

An active release epoch is an open tracker issue carrying the `epoch` label and
either the `release` or `quickstart` label. Before opening the next release or
quickstart epoch, add it to the policy config as an active epoch:

```json
{
  "id": "m80-next",
  "status": "active"
}
```

Retired epochs remain in the config only when they have `status: "retired"` and
a nonempty `reason`. A tracker issue that still matches the active-epoch rule
cannot be configured as retired. The verifier fails closed on omitted active
epochs, missing configured active epochs, duplicate epoch ids, unknown statuses,
or retired entries without reasons. For local one-off checks,
`scripts/verify-release-tracker-policy.py --epic <id>` still lints a single
epoch without using the config; CI uses the config-backed path.

The policy intentionally keeps scaffolded work separate from verified close.
Unit tests, fake release servers, hostless fixtures, and validators may land and
remain useful before the final substrate proof exists. The bead stays open
until a committed artifact records the real proof named by the bead.

Final release-epoch closure uses a machine-readable close matrix artifact. The
matrix is a JSON object with `kind: "release_final_close_matrix"` and
`schema_version: 1`:

```json
{
  "schema_version": 1,
  "kind": "release_final_close_matrix",
  "epoch_id": "m80-o3uh9",
  "tracker_digest": "sha256:<64 lowercase hex>",
  "generated_at": "2026-05-21T00:00:00Z",
  "rows": [
    {
      "id": "m80-o3uh9.1",
      "status": "closed",
      "behavior_doc": "docs/behaviors/release/example.md",
      "test_command": "python3 scripts/test-release-tracker-policy.py",
      "requires_verified_close": true,
      "requires_real_substrate": false
    }
  ]
}
```

Each row describes one epoch descendant. `behavior_doc` and `proof_artifact`
paths, when present, must be relative repository paths and must not escape the
tree. A row must include either `behavior_doc` plus `test_command`,
`proof_artifact`, or `exception_reason` for an accepted deferred/tombstone
entry.
`requires_verified_close` records whether the descendant was governed by this
policy; `requires_real_substrate` records whether its close depended on a
real-substrate proof rather than hostless or unit-test evidence. The schema
validator fails closed on unknown top-level keys, unknown row keys, malformed
timestamps, malformed tracker digests, empty rows, absolute paths, escaping
paths, and unknown schema versions.

For closed epoch verification, the matrix rows must match the epoch descendants
in `.beads/issues.jsonl`. Missing descendants, unknown rows, stale statuses,
open descendants, proof-shaped rows without `requires_verified_close: true`,
and substrate-gated rows without `requires_real_substrate: true` fail the
policy before CI can pass.

When a configured release or quickstart epoch is closed, its close reason must
cite a committed final close matrix with
`verified: <artifact-path> @ <commit-sha>`. A generic quickstart proof artifact
is not enough for epoch closure; the epoch-level artifact must be the final
matrix shape above. Parent epochs keep `requires-verified-close` until that
matrix is committed.

Relevant tests:

- `scripts/test-release-tracker-policy.py`
- CI `release script tests` runs both the fixture tests and the live tracker
  lint.
