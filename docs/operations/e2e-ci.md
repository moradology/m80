# Privileged E2E CI

`.github/workflows/e2e-privileged.yml` runs the real-KVM smoke and ignored
privileged test battery on a self-hosted runner labeled `self-hosted` and
`kvm`. GitHub-hosted runners do not expose `/dev/kvm`, so this workflow is the
CI lane for kernel, jailer, namespace, cgroup, and Firecracker launch behavior.

## Runner Contract

Register only runners that satisfy
[`e2e-privileged-runner.md`](e2e-privileged-runner.md):

- Linux x86_64 with `/dev/kvm` readable and writable by the runner user;
- passwordless `sudo -n` for iptables, netlink, run-root cleanup, and smoke;
- KSM disabled (`/sys/kernel/mm/ksm/run` is `0` when present);
- Firecracker and jailer available at the paths used by the smoke scripts;
- `M80_KERNEL_IMAGE` and `M80_ROOTFS_IMAGE` pointing at real guest artifacts, or
  artifacts staged at the workflow defaults under `/tmp/m80-build-current`;
- a run root on a filesystem that permits device nodes.

The workflow does not download secrets. Top-level permissions are read-only.
Do not attach this runner to untrusted fork pull requests. The PR trigger runs
only for same-repository pull requests with the `run-e2e` label.

## Triggers

- `workflow_dispatch` for operator-driven runs.
- Weekly scheduled run on the default branch.
- Tag pushes matching `v*`.
- Same-repository pull requests after a maintainer adds `run-e2e`.

Manual inputs:

- `package`: Rust package passed to `scripts/run-e2e.sh`; default
  `m80-firecracker`.
- `timeout_seconds`: per-test timeout; default `180`.
- `run_smoke`: `true` or `false`; default `true`.
- `smoke_mode`: `full` or `launch-only`; default `full`.
- `upload_run_dirs`: archive the run root after failure; default `true`.

Repository or organization variables can override:

- `M80_E2E_RUN_ROOT`
- `M80_RUN_ROOT`
- `M80_KERNEL_IMAGE`
- `M80_ROOTFS_IMAGE`
- `M80_JAILER_HARDEN_BIN`

## What Runs

The job checks the substrate first: `/dev/kvm`, `sudo -n`, `ip`, `iptables`,
KSM disabled, and kernel/rootfs artifact paths. It then runs
`scripts/smoke.sh`, followed by:

```sh
scripts/run-e2e.sh --package "$M80_E2E_PACKAGE" --timeout "$M80_E2E_TIMEOUT_SECONDS" --json
python3 scripts/validate-e2e-report.py "$RUNNER_TEMP/m80-e2e-report.json"
```

`scripts/run-e2e.sh` performs the pre-run stale-state cleanup, per-test leak
check, structured skip classification, and JSON report emission described in
the adjacent E2E operation docs.

## Artifacts

Every run uploads:

- `m80-e2e-report.json`
- `m80-e2e-summary.txt`
- `m80-e2e-substrate.txt`
- `m80-smoke.log`
- `m80-e2e-runner-diagnostics.txt`

Failed runs also try to upload `m80-e2e-run-root.tgz` when
`upload_run_dirs=true`.

## Promotion Policy

This workflow is initially informational for pull requests. Do not mark it as
a required branch-protection check until scheduled and manual runs are stable
on the selected self-hosted runner. Tag runs are release evidence, but they do
not replace the separate public-install freshness and release-publish lanes.
