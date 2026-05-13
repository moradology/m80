# Cold Restore Latency

Bead: `m80-jp6ik.43`.

Raw artifact:

- `crates/m80-firecracker/benches/snapshots/cold-restore-N50.json`

## Method

Run date: 2026-05-13.

Command:

```sh
N=50 FILE_READ_SAMPLES=10 \
  IMAGE_BUILD_DIR_MINIMAL=/tmp/m80-build/minimal-jp6ik42 \
  bash scripts/bench-restore-cold.sh
```

Host: Linux 6.17.0-22-generic x86_64, 48 logical CPUs. Runtime used
Firecracker v1.15.1, real `/dev/kvm`, real jailer, and
`/tmp/m80-build/minimal-jp6ik42` with the stripped kernel from `m80-jp6ik.23`.

The benchmark captures one golden snapshot, then runs 50 warm-cache restores
and 50 cold-cache restores. Cold-cache mode runs `sync; echo 3 >
/proc/sys/vm/drop_caches` before each restore. The same artifact also times
direct warm and cold reads of `vm.snap` and `mem.snap` so the snapshot-file
read ceiling is visible separately from Firecracker's restore critical path.

## Results

| cache mode | N | restore P50 | P95 | P99 | max |
|---|---:|---:|---:|---:|---:|
| warm | 50 | 55.316 ms | 70.884 ms | 71.876 ms | 75.322 ms |
| cold | 50 | 93.945 ms | 97.654 ms | 98.395 ms | 107.082 ms |

Cold-cache restore adds 38.629 ms P50 over warm-cache restore.

| phase | warm P50 | cold P50 | delta |
|---|---:|---:|---:|
| `phase_3_storage_prep` | 8.756 ms | 11.537 ms | +2.781 ms |
| `phase_9_jailer_launch` | 15.722 ms | 31.899 ms | +16.177 ms |
| `phase_restore_load` | 2.650 ms | 3.252 ms | +0.602 ms |
| `phase_restore_probe_exec_channel` | 18.408 ms | 37.984 ms | +19.576 ms |
| `phase_restore_snapshot_bind` | 0.161 ms | 0.167 ms | +0.006 ms |

| file | size | warm direct-read P50 | cold direct-read P50 | cold-warm delta |
|---|---:|---:|---:|---:|
| `vm.snap` | 14,271 bytes | 0.009 ms | 0.642 ms | +0.633 ms |
| `mem.snap` | 1,073,741,824 bytes | 191.239 ms | 806.347 ms | +615.108 ms |

## Interpretation

`mem.snap` dominates a full direct read of the snapshot pair, but Firecracker's
restore path does not synchronously read the full 1 GiB memory file before
returning a ready VM. The measured `phase_restore_load` cold-warm delta is only
0.602 ms P50. The user-visible cold restore tax appears mostly in
`phase_restore_probe_exec_channel` (+19.576 ms P50) and `phase_9_jailer_launch`
(+16.177 ms P50).

This makes the `m80-jp6ik.34` fadvise hypothesis more precise: priming
`mem.snap` may still help the pages touched by the restored guest probe, but
the N=50 baseline does not support a model where restore readiness blocks on a
full memory snapshot read. A realistic P50 recovery target for snapshot-file
priming is therefore bounded by the 38.629 ms cold-warm restore delta, not by
the 615.108 ms full-file cold-read delta for `mem.snap`.

For future close gates, use this artifact as the baseline. A successful
`m80-jp6ik.34` run should compare against `cold.p50_us = 93945` and should
show which of `phase_restore_load` or `phase_restore_probe_exec_channel`
actually moved.
