# Privileged E2E Workflow

Beads: `m80-16hx7.7`, `m80-s3r28.2`, `m80-25mdp`

The privileged E2E workflow is the self-hosted CI lane for real-KVM behavior.
It targets runners labeled `self-hosted` plus the configured runner label
(`kvm` by default), keeps GitHub permissions read-only, and refuses fork
pull-request execution by requiring same-repository PRs plus the `run-e2e`
label.

Contract:

- substrate checks prove `/dev/kvm`, `sudo -n`, `ip`, `iptables`, and guest
  artifact paths before the battery runs;
- `workflow_dispatch` accepts `runner_label`, validates it as
  `[A-Za-z0-9_.-]+`, and records the selected label in substrate diagnostics;
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
