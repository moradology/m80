# Privileged E2E Test Selection

`scripts/run-e2e.sh` is the entrypoint for ignored privileged tests. It builds
the selected package's integration tests, reads each test's structured
`#[ignore = "..."]` reason, detects the current host, and either runs the test
or reports a skip reason.

## Taxonomy

Every ignored Rust test must use one or more space-separated tokens:

| Token | Meaning |
| --- | --- |
| `requires-kvm` | Needs `/dev/kvm` and real Firecracker execution. |
| `requires-root` | Needs root or passwordless `sudo -n`. |
| `requires-network-namespace` | Needs `ip`/netns or CAP_NET_ADMIN-style host networking. |
| `requires-cgroup-v2` | Needs a writable unified cgroup v2 hierarchy. |
| `requires-artifacts` | Needs real m80 kernel/rootfs or guest artifacts. |
| `requires-external-network` | Needs `M80_RUN_EXTERNAL_NETWORK_E2E=1`. |
| `requires-malicious-artifacts` | Needs `M80_MALICIOUS_ARTIFACT_DIR`. |
| `requires-docker` | Needs a working Docker daemon. |
| `requires-mount-namespace` | Needs mount namespace creation. |
| `requires-loop-device` | Needs loop-device or loop-mount access. |
| `requires-snapshot-support` | Needs Firecracker snapshot support. |
| `requires-debugfs` | Needs `debugfs`. |
| `requires-pmem` | Needs pmem/erofs/DAX fixture artifacts. |
| `measurement` | Measurement-shaped; skipped unless `M80_RUN_MEASUREMENT_E2E=1`. |
| `slow` | Slow by design; still must combine with concrete resource tokens. |
| `manual` | Deliberately manual; the runner reports `manual-ignore`. |

Do not write prose in `#[ignore]`. The taxonomy is enforced by
`scripts/test-ignored-taxonomy.py`.

## Commands

List the selected package's ignored tests and why they would skip:

```sh
scripts/run-e2e.sh --list --json | jq '.results[] | {status,test,reason}'
```

Validate the machine-readable report contract:

```sh
scripts/run-e2e.sh --list --json > /tank/tmp/m80-e2e-report.json
python3 scripts/validate-e2e-report.py /tank/tmp/m80-e2e-report.json
```

Run the default privileged battery one test at a time:

```sh
scripts/run-e2e.sh
```

Run a different package:

```sh
scripts/run-e2e.sh --package m80-cli
```

Opt into external-network and measurement-shaped tests explicitly:

```sh
M80_RUN_EXTERNAL_NETWORK_E2E=1 scripts/run-e2e.sh
M80_RUN_MEASUREMENT_E2E=1 scripts/run-e2e.sh --package m80-storage
```

Before a real run, the wrapper invokes `scripts/e2e-reap.sh` to remove stale
m80-owned residue from previous failed runs. Set `M80_E2E_SKIP_REAPER=1` only
when debugging the reaper itself.

The JSON schema is documented in [`e2e-reporting.md`](e2e-reporting.md).
