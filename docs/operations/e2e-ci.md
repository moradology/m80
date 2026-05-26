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
- Rust toolchain and smoke build tools on PATH: `cargo`, `rustc`, `rg`, and
  `unsquashfs`;
- Firecracker and jailer available at the paths used by the smoke scripts;
- for `run_smoke=true` with `smoke_mode=full`, enough toolchain support for
  `scripts/smoke.sh` to build guest artifacts at the configured paths;
- when smoke is skipped or `smoke_mode=launch-only`, `M80_KERNEL_IMAGE` and
  `M80_ROOTFS_IMAGE` pointing at real guest artifacts, or artifacts staged at
  the workflow defaults under `/tmp/m80-build-current`;
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
- `external_network`: `true` or `false`; default `false`. Set to `true` to
  export `M80_RUN_EXTERNAL_NETWORK_E2E=1` and run tests tagged
  `requires-external-network`.
- `upload_run_dirs`: archive the run root after failure; default `true`.
- `runner_label`: self-hosted runner label to target; default `kvm`. For a
  disposable L1 run, set this to the unique label used when registering the
  ephemeral runner, for example `m80-e2e-20260526T190000Z`.
- `pull_number`: optional pull request number. When `target_sha` is set, the
  workflow fetches `refs/pull/<pull_number>/head` before checking out the exact
  target SHA. This is the preferred path for proving a fork PR commit.
- `target_sha`: optional exact 40-character commit SHA to check out. Invalid
  SHAs fail before tests run. If `pull_number` is omitted, the workflow tries a
  direct public fetch of that SHA from the repository remote.

Repository or organization variables can override:

- `M80_E2E_RUN_ROOT`
- `M80_RUN_ROOT`
- `M80_IMAGE_KIND`
- `M80_ARTIFACT_DIR`
- `M80_KERNEL_IMAGE`
- `M80_ROOTFS_IMAGE`
- `M80_FIRECRACKER_SECCOMP_FILTER`
- `M80_BIN`
- `M80_JAILER_HARDEN_BIN`
- `M80_NET_HELPER_BIN`

`M80_E2E_RUNNER_LABEL` can also override the default runner label for scheduled
or tag-triggered runs. Keep this set to `kvm` unless a disposable runner has
already been registered with a unique label.

## Disposable L1 Runners

The durable `kvm` runner is acceptable for maintainer-triggered proof, but it
is not the end state for arbitrary external code. For untrusted or higher-risk
runs, use a fresh L1 VM and a one-job GitHub runner label:

```sh
label="m80-e2e-$(date -u +%Y%m%dT%H%M%SZ)"
work_root="/tank/tmp/$label"

scripts/spawn-l1-runner.sh create \
  --name "$label" \
  --work-root "$work_root"

scripts/register-l1-github-runner.sh \
  --l1-name "$label" \
  --l1-work-root "$work_root" \
  --runner-name "$label" \
  --runner-label "$label"

gh workflow run e2e-privileged.yml \
  -r main \
  -f runner_label="$label" \
  -f pull_number=123 \
  -f target_sha=0123456789abcdef0123456789abcdef01234567 \
  -f external_network=true \
  -f run_smoke=true \
  -f smoke_mode=full \
  -f timeout_seconds=180

run_id="$(gh run list --workflow e2e-privileged.yml --branch main --limit 1 --json databaseId --jq '.[0].databaseId')"
gh run watch "$run_id" --exit-status
scripts/spawn-l1-runner.sh destroy --name "$label" --work-root "$work_root"
```

`scripts/register-l1-github-runner.sh` obtains a repository runner
registration token through `gh`, installs the current `actions/runner` release
inside the L1 if needed, registers with `config.sh --ephemeral`, and starts the
runner process. GitHub deregisters an ephemeral runner after one job; destroying
the L1 removes the working directory and VM disk. The registration-token call
requires repository runner administration authority, so this is still an
operator-mediated control-plane step rather than something arbitrary PR code can
start by itself.

## What Runs

The job checks the substrate first: `/dev/kvm`, `sudo -n`, `ip`, `iptables`,
`cargo`, `rustc`, `rg`, `unsquashfs`, `mkfs.erofs`, KSM disabled,
kernel/rootfs/seccomp artifact paths, and the m80 host-binary paths. It then
runs `scripts/smoke.sh`,
which refreshes `m80-guestd` from the checked-out commit and rebuilds the guest
image when the manifest daemon hash is stale. When `external_network=true`,
the job sets `M80_RUN_EXTERNAL_NETWORK_E2E=1` for the ignored-test wrapper so
external DNS, HTTP, and ICMP probes are selected instead of skipped. The smoke
is followed by:

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
