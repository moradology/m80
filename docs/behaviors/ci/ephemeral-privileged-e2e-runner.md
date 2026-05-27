# Ephemeral Privileged E2E Runner

Bead: `m80-25mdp`

The privileged E2E workflow can target a per-run self-hosted runner label
instead of the durable `kvm` label. Manual dispatch requires `runner_label`;
the job uses that exact input and rejects labels that do not start with
`m80-e2e-`. There is no repository-variable or `kvm` fallback. The workflow has
no schedule, pull-request, or tag trigger, so privileged execution cannot bypass
the broker-created runner path.

`scripts/register-l1-github-runner.sh` registers an already-created L1 VM as a
repository self-hosted runner with `config.sh --ephemeral`, labels it with the
caller-provided unique label, `m80-privileged-e2e`, and `kvm`, starts the
runner process, and waits until GitHub reports the runner online. The operator
then dispatches `.github/workflows/e2e-privileged.yml` with that same label.
GitHub assigns at most one job to that runner, and the operator destroys the L1
VM after logs and artifacts are uploaded.

This does not make arbitrary fork PRs safe by itself. The remaining trusted
control-plane step is runner creation: obtaining a repository runner
registration token still requires runner-administration authority, and the
ephemeral L1 must be created by a maintainer-controlled host or service.

Tests:

- `python3 scripts/test-workflow-policy.py`
- `python3 scripts/lint-github-workflows.py`
- `python3 scripts/run-actionlint.py --workflow-dir .github/workflows`
- `bash -n scripts/register-l1-github-runner.sh`
