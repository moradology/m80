# Behavior: Snapshot First-Line Sizing

**Bead:** `m80-0tf.4.1`
**Date:** 2026-05-06
**Updated:** 2026-05-13 (`m80-jp6ik.29` changed default memory to 512 MiB)
**Test:** `crates/m80-firecracker/tests/snapshot/first_line_sizing.rs`
**Raw data:** `docs/behaviors/snapshot/first-line-sizing-latency.json`

## 1-vcpu-512-mib

Snapshot timing proofs use m80's first-line Firecracker shape: 1 vCPU and
512 MiB memory. The values are exposed by `m80-firecracker` as
`FIRST_LINE_VCPU_COUNT` and `FIRST_LINE_MEM_SIZE_MIB`, and omitted
`SandboxConfig::vcpu_count` / `SandboxConfig::mem_size_mib` resolve to those
same values during the preboot `/machine-config` PUT.

This does not reintroduce predecessor's old fixed-sizing preflight gate.
`SandboxConfig` still accepts caller-provided sizing for general VM launches.
The narrower rule here is that snapshot restore and warm-pool latency evidence
must use the exported first-line constants rather than a hard-coded
benchmark-only shape.

predecessor source: `docs/gates/stage-g-firecracker-snapshot-restore-contract.md`
§Contract rule 11.

## measured-idle-sample

On 2026-05-06, `snapshot_restore_latency` was run with `N=10` against the
then-current 1 vCPU / 1024 MiB first-line shape and the existing minimal
artifact pair at
`/tmp/m80-build/minimal-perf-20260505d/`. The host run-root was
`/var/lib/m80-run`.

| host load | N | P50 | P95 | max | mean |
|---|---:|---:|---:|---:|---:|
| idle | 10 | 47.777 ms | 49.355 ms | 55.150 ms | 47.873 ms |

This is a bounded sizing proof, not the broad loaded-host benchmark. The
loaded-host restore and warm-pool latency cells remain owned by the warm-pool
performance lane.
