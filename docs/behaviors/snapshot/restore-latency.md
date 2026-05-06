# Behavior: Snapshot Restore Ready Latency

**Bead:** `m80-rrp.3.6`
**Date:** 2026-05-05
**Bench file:** `crates/m80-firecracker/benches/snapshot_restore_latency.rs`
**Raw data:** `docs/behaviors/snapshot/restore-latency.json`

## Summary

`Sandbox::launch_from_snapshot` restores a captured Minimal VM and returns once
`phase_restore_probe_exec_channel` has confirmed guestd's exec listener is
live. The timer starts immediately before `launch_from_snapshot` and stops
when it returns `RunningSandbox`.

Real-KVM results:

| host load | N | P50 | P95 | max | mean |
|---|---:|---:|---:|---:|---:|
| idle | 50 | 274.204 ms | 280.132 ms | 289.409 ms | 269.176 ms |
| loaded (`stress-ng --cpu $(nproc)`) | 50 | 444.972 ms | 588.221 ms | 1834.719 ms | 497.661 ms |

Compared with the stock post-pivot cold-launch checkpoint
(`minimal/idle` P50 1517 ms), idle snapshot restore is about 5.5x faster at
P50 and saves roughly 1243 ms per allocation. Loaded restore still preserves a
large median win, but its tail shows host contention: three samples exceeded
1 s.

## Notes

The capture path bind-mounts the caller's snapshot directory into the
Firecracker jail at `/snapshot`, then passes `/snapshot/vm.snap` and
`/snapshot/mem.snap` to the Firecracker API. Without that bind, jailed
Firecracker cannot see host paths outside the chroot and snapshot creation
fails with `Cannot perform open on the snapshot backing file`.
