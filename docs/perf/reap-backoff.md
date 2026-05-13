# Reap Backoff Delta

Bead: `m80-jp6ik.16`.

Raw artifact:

- `crates/m80-firecracker/benches/snapshots/reap-backoff-delta.json`

## Method

Run date: 2026-05-13.

Baseline command, before the reap backoff change:

```sh
N=50 KIND=minimal SKIP_LOADED=1 KERNEL_KIND=stripped \
  IMAGE_BUILD_DIR_MINIMAL=/tmp/m80-build/minimal-jp6ik42 \
  M80_BIN=./target/release/m80 ./scripts/bench-cold-launch.sh
```

After replacing the fixed 20 ms post-SIGKILL poll sleep with a 1/2/4/8/16/20 ms
bounded backoff, the release CLI was rebuilt and the same command was run
again.

Focused test:

```sh
cargo test -p m80-firecracker reap_ --lib
```

## Results

| mode | wallclock P50 | wallclock P95 | `stop_bounded` P50 | `stop_bounded` P95 | `stop_bounded` P99 |
|---|---:|---:|---:|---:|---:|
| fixed 20 ms poll | 1595 ms | 1695 ms | 65.785 ms | 71.629 ms | 76.982 ms |
| reap backoff | 1598 ms | 1697 ms | 56.669 ms | 67.906 ms | 69.296 ms |

The stop-path P50 improved by 9.116 ms, or 13.86%. The rough 15 ms estimate
did not fully land on this host/run, but the fixed 20 ms minimum wait is gone
and the existing two-second reap timeout is preserved.

## Interpretation

This is a stop-path cleanup, not a launch critical-path optimization. The
`stop_bounded` phase improved at P50/P95/P99, while total launch wallclock
stayed effectively flat. That shape matches the intent: the change trims
post-exec teardown wait without claiming a boot-speed win.
