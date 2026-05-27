# Privileged E2E Workflow

Beads: `m80-16hx7.7`, `m80-s3r28.2`, `m80-25mdp`, `broker-jo7.2`

The privileged E2E workflow is the self-hosted CI lane for real-KVM behavior.
It targets runners labeled `self-hosted`, `m80-privileged-e2e`, `kvm`, plus a
broker-minted unique `m80-e2e-*` label. The workflow has only a
`workflow_dispatch` trigger, keeps GitHub permissions read-only, and has no
tag, schedule, or pull-request fallback to the durable `kvm` runner.

Contract:

- substrate checks prove `/dev/kvm`, `sudo -n`, `ip`, `iptables`, and guest
  artifact paths before the battery runs;
- `workflow_dispatch` requires `runner_label`, validates it as
  `m80-e2e-*`, and records the selected label in substrate diagnostics;
- `workflow_dispatch` accepts optional `pull_number` and `target_sha` inputs
  for disposable broker runs. `target_sha` must be a full 40-character hex SHA,
  `pull_number` must be a positive integer when present, and the workflow checks
  out the exact requested SHA after fetching either the public PR ref or the
  direct SHA from the repository remote;
- substrate diagnostics record the requested pull number, requested target SHA,
  and actual checked-out SHA before smoke or ignored tests run;
- `scripts/smoke.sh` runs before ignored tests unless an operator disables it
  for wrapper debugging;
- `workflow_dispatch` has an `external_network` input that selects
  `requires-external-network` tests by exporting
  `M80_RUN_EXTERNAL_NETWORK_E2E=1`;
- `scripts/run-e2e.sh --json` emits the machine-readable report and
  `scripts/validate-e2e-report.py` validates it in the same job;
- artifacts are uploaded with `if: always()`, including runner diagnostics and
  failure run-root archives when enabled.

Local verification:

- `python3 scripts/lint-github-workflows.py`
- `python3 scripts/run-actionlint.py --workflow-dir .github/workflows`

Closure note: the workflow scaffold is not enough to close `m80-16hx7.7` until
one self-hosted runner execution has produced a green artifact set.
