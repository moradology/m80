# Release readiness lanes

`release-readiness-lanes.json` is the lane contract for the release readiness
gate. Local CI lanes write normalized JSON receipts with
`scripts/release_readiness_receipt.py` before the aggregate gate reads them.

Each local lane receipt records:

- `lane_id`, `lane_kind`, and `proof_kind` from the configured lane;
- `status`, `release_tag`, `commit_sha`, and optional `workflow_run_id`;
- `substrate.kind` and whether the artifact came from a fixture;
- the artifact path relative to the artifact root, its `sha256:<hex>` digest,
  and size;
- the configured digest field for the lane, equal to the artifact digest;
- a remediation command or bead id for blocking failures.

The writer verifies the receipt after writing it. It fails closed for unknown
lane ids, unknown statuses, stale release tag or commit, missing configured
digest fields, unsupported substrate claims, and malformed remediation commands.
Fixture output cannot claim a stronger substrate such as GitHub Actions,
public GitHub, or real KVM.

These receipts are inputs to the aggregate readiness decision. They do not
replace the underlying checks: the workflow policy linter, release-integrity
verification, and quickstart proof verifier still run and produce their own
artifacts.

`scripts/release_readiness_decision.py` consumes the lane receipts as explicit
`lane_id=path` inputs and writes the aggregate decision. Missing required lanes,
stale tag or commit, authenticated public-access proof, blocking statuses, and
fixture substitution for real-substrate lanes fail before a publish hook can
consume the decision.
