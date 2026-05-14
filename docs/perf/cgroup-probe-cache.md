# Cgroup Probe Cache

Date: 2026-05-14

Bead: `m80-jp6ik.36`

## Setup

The code change for this bead caches `m80_cgroup::Subtree::probe()` in a
process-wide `OnceLock`. The first probe still validates the host cgroup v2
mount and root controller visibility; later launch-path probes replay the
cached typed result.

## Evidence

Committed current-run artifact:

- `crates/m80-firecracker/benches/tokio-runtime-cost.jsonl`
- 20 samples of `phase_5_cgroup_probe`
- all 20 samples recorded `elapsed_us=0`

Committed older baselines:

- `crates/m80-firecracker/benches/baseline.json` records
  `phase_5_cgroup_probe` P50 `2041us`.
- `docs/behaviors/lifecycle/cold-launch-phase-profile.json` records six
  `phase_5_cgroup_probe` cells: `1918us`, `2519us`, `1957us`, `2658us`,
  `1904us`, and `1887us`; median `1937.5us`.

## Interpretation

The launch path now rounds repeat process-local cgroup probe cost down to the
phase recorder's `0us` bucket after the first successful probe. Against the
older committed baselines, the measured P50 reduction is roughly `1.9-2.0ms`,
which exceeds the bead's expected `15-40us` drop.

This does not remove cgroup validation. It moves the host-static mount and root
controller probe to the first process-local call, then reuses that result for
later launches in the same m80 process.

## Verification

Previously recorded implementation verification:

- `cargo test -p m80-cgroup probe_cache --lib`
- `cargo test -p m80-cgroup public_probe --lib -- --nocapture`
- `cargo test -p m80-cgroup`
- `cargo test -p m80-cgroup --no-run`
- `cargo fmt -p m80-cgroup --check`
- `git diff --check`
