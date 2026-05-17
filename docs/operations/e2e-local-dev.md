# Privileged E2E Local Development

This is the operator workflow for running the real-KVM test battery from a
developer checkout. It assumes the behavior tests already exist; this document
only explains how to run them without leaking host state or misreading skipped
tests.

## Prerequisites

Read `docs/ops/host-setup.md` first. The privileged battery needs:

- Linux with `/dev/kvm` present.
- Passwordless `sudo` for the test command, or run the wrapper as root.
- A run root on a filesystem that allows device nodes. Use `/var/lib/m80-r`
  or `/var/lib/m80-run`, not `/tmp` when `/tmp` is mounted `nodev`.
- Firecracker, jailer, `m80-jailer-harden`, and real guest artifacts.
- A jail UID/GID that exists on the host. If UID/GID `3000` is not present,
  export `M80_JAIL_UID=$(id -u)` and `M80_JAIL_GID=$(id -g)` for local smoke
  runs.
- Membership in the `kvm` group when non-root VMM processes must open
  `/dev/kvm`.

For the default artifact location used by the local smoke commands:

```sh
export M80_KERNEL_IMAGE=/opt/m80/artifacts/vmlinux
export M80_ROOTFS_IMAGE=/opt/m80/artifacts/output.ext4
export M80_E2E_RUN_ROOT=/var/lib/m80-r
export M80_JAIL_UID=$(id -u)
export M80_JAIL_GID=$(id -g)
sudo mkdir -p "$M80_E2E_RUN_ROOT"
```

Build or refresh artifacts using the flow in `README.md` "Build The
Artifacts". If you keep a local artifact cache elsewhere, point
`M80_KERNEL_IMAGE` and `M80_ROOTFS_IMAGE` at that cache before running tests.

## Daily Workflow

Build the host-side helper used by the wrapper:

```sh
cargo build -p m80-jailer-harden
```

Run one privileged test directly when debugging a behavior:

```sh
cargo test -p m80-firecracker --test lifecycle_failure_real_kvm --no-run
sudo -n env \
  M80_KERNEL_IMAGE="$M80_KERNEL_IMAGE" \
  M80_ROOTFS_IMAGE="$M80_ROOTFS_IMAGE" \
  M80_RUN_ROOT=/var/lib/m80-run \
  M80_JAIL_UID="$M80_JAIL_UID" \
  M80_JAIL_GID="$M80_JAIL_GID" \
  target/debug/deps/lifecycle_failure_real_kvm-* \
  --ignored api_socket_timeout_cleans_partial_state --exact --nocapture
```

Run the detected privileged battery one test at a time:

```sh
scripts/run-e2e.sh
```

List what the wrapper would run or skip:

```sh
scripts/run-e2e.sh --list
```

Emit machine-readable output:

```sh
scripts/run-e2e.sh --json | jq '.summary, .results[] | {status,test,reason}'
```

The wrapper detects missing `sudo`, `/dev/kvm`, kernel/rootfs artifacts,
`m80-jailer-harden`, optional malicious-guestd artifacts, and optional external
network opt-in. It reports skipped tests with explicit reason strings instead
of leaving `#[ignore]` output for humans to reverse-engineer.

## Cleanup Recipes

Before a large local run, clear stale m80 residue from previous failed
privileged tests:

```sh
sudo find /var/lib/m80-r /var/lib/m80-run -mindepth 1 -maxdepth 1 -mtime +1 -exec rm -rf {} +
ip link show | grep -E 'tfc[0-9a-f]{12}|m80-br' || true
sudo iptables -S FORWARD | grep m80 || true
```

If a leftover tap or bridge is clearly m80-owned and no test is running, delete
it explicitly:

```sh
sudo ip link delete <tap-or-bridge-name>
```

If iptables is in a weird state, inspect handles before deleting anything:

```sh
sudo iptables -L FORWARD -n --line-numbers | grep m80
```

Use `scripts/run-e2e.sh --list` after cleanup to confirm the environment
detects the expected runnable subset.

## Debugging Failures

Most real-KVM failures leave useful artifacts under the run root:

- `diagnostics.jsonl` records phase starts/completions and typed errors.
- `failure_summary.json` records failed launch phase, `FcError` variant, and
  display text when launch fails before `RunningSandbox` exists.
- `console.log` carries guest serial output and early boot markers.

For launch failures, inspect preserved directories first:

```sh
sudo find /var/lib/m80-run/.preserved /var/lib/m80-r/.preserved -maxdepth 2 -type f \
  \( -name failure_summary.json -o -name diagnostics.jsonl -o -name console.log \) -print
```

Then match the failing test to the behavior document under `docs/behaviors/`.
For example, launch failure preservation is documented in
`docs/behaviors/observability/launch-failure-summary.md`.

## Verified Local Smoke

On this host, the following privileged command passed with the real artifacts
under `/opt/m80/artifacts`:

```sh
sudo -n env \
  M80_KERNEL_IMAGE=/opt/m80/artifacts/vmlinux \
  M80_ROOTFS_IMAGE=/opt/m80/artifacts/output.ext4 \
  M80_RUN_ROOT=/var/lib/m80-run \
  M80_JAIL_UID=1000 \
  M80_JAIL_GID=1000 \
  target/debug/deps/lifecycle_failure_real_kvm-a9e86aad47ca4dcf \
  --ignored api_socket_timeout_cleans_partial_state --exact --nocapture
```

Result: 1 passed, 0 failed.
