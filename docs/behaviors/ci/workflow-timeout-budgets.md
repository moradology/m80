# Workflow Timeout Budgets

Behavior capture for bead `m80-o3uh9.13.35`.

## Contract

`docs/behaviors/ci/workflow-timeout-budgets.json` is the checked source of
truth for workflow job timeout budgets that are documented in the release
runbook.

Every workflow scoped as `release-authority`, `latest-freshness`, or `proof` in
`docs/behaviors/ci/workflow-policy-scope.json` must have one timeout budget
entry per job. Normal jobs must set `timeout-minutes` to the configured value.
Reusable workflow jobs must carry a YAML comment
`m80-lint: reusable-timeout-minutes=N` whose value matches the configured
budget. All configured budgets must be between 1 and 120 minutes.

The runbook timeout table is not prose-owned. It must match the configured
budget rows exactly, including the latest-freshness jobs.

## Enforcement

`scripts/lint-github-workflows.py` loads the timeout budget config alongside
the workflow scope config. It fails closed when:

- the config is missing, malformed, has unknown fields, or uses an unsupported
  schema version;
- a guarded workflow job has no configured budget;
- a configured workflow/job is missing;
- a YAML `timeout-minutes` value or reusable timeout marker differs from the
  configured value;
- the release runbook timeout table drifts from the config.

## Verification

`scripts/test-workflow-policy.py` covers missing budget config entries, YAML
timeout mismatches, reusable timeout marker mismatches, runbook table drift, and
the clean repository workflow set.
