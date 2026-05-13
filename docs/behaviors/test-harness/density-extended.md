# Extended Density Harness

Bead: `m80-jp6ik.42`.

The extended density harness is `scripts/bench-density-extended.sh`. It is a
thin orchestration layer over `scripts/bench-cold-launch.sh`: every cell uses
the same real `m80 run` path, and the wrapper only selects `CONCURRENT`,
`EGRESS`, `N`, `WARMUP`, `KIND`, and `KERNEL_KIND`.

The committed capacity artifact is separate from the append-only cold-launch
CSV:

- `crates/m80-firecracker/benches/density-extended.csv`
- `crates/m80-firecracker/benches/snapshots/density-extended.json`

The interpretation lives in `docs/perf/density-extended.md`. The shell
plumbing regression test is `scripts/test-bench-harness.sh`; it checks that the
new wrapper has a dry-run plan and that `EGRESS=outbound` reaches the
underlying cold-launch plan.
