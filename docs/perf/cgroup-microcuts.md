# Cgroup Micro-Cuts

Date: 2026-05-14

Bead: `m80-jp6ik.32`

## Verification

Implementation tests:

```sh
cargo test -p m80-cgroup kill_cgroup --lib
cargo test -p m80-cgroup
rustfmt --edition 2021 --check \
  crates/m80-cgroup/src/lib.rs \
  crates/m80-firecracker/src/storage_prep.rs
```

Real host cgroup cleanup smoke:

```sh
sudo env PATH="$PATH" \
  cargo test -p m80-cgroup \
  tests::cgroup_drop_with_live_procs_uses_cgroup_kill_then_rmdir \
  --lib -- --include-ignored --exact --nocapture
```

Result: passed on the writable cgroup v2 host.

The first real-host run exposed a correctness gap: `Subtree::Drop` wrote
`cgroup.kill` and immediately attempted `rmdir`, but the kernel can take a
short interval to drain `cgroup.procs`. The fix now waits briefly for
`cgroup.procs` to become empty before removing the leaf. The second real-host
run passed.

## Phase Evidence

Current committed artifacts:

- `crates/m80-firecracker/benches/tokio-runtime-cost.jsonl`: 20 samples,
  `phase_5b_cgroup_create` P50 `19938.5us`.
- `crates/m80-firecracker/benches/snapshots/cpu-governor-sweep.json`:
  `phase_5b_cgroup_create` P50 `20306us` under `performance`, `19310us`
  under `ondemand`.

Older committed baseline:

- `crates/m80-firecracker/benches/baseline.json`: `phase_5b_cgroup_create`
  P50 `18650us`.

## Interpretation

The correctness half of the bead is now verified on the real writable cgroup
hierarchy: live-process teardown uses `cgroup.kill`, waits for the kernel to
empty `cgroup.procs`, and removes the leaf.

The small `phase_5b_cgroup_create` performance target is not cleanly proven by
the current committed artifacts. The available runs are noisy and differ by
host governor and surrounding benchmark context; they do not show a stable
`~0.5ms` improvement over the older baseline. Treat the performance side as a
measured non-win, and keep the change for correctness and reduced redundant
controller probing.
