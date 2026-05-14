# Page-Cache Priming

## Launch Artifacts

`m80-jp6ik.4` tested `posix_fadvise(POSIX_FADV_WILLNEED)` for the cold-launch
artifact set: Firecracker, jailer, `m80-jailer-harden`, the kernel, and the
rootfs. The trial implementation was backed out because it did not reduce the
cold-isolated launch tax.

Commands and verification from the trial:

```sh
cargo test -p m80-preflight
cargo test -p m80-storage
cargo build --release -p m80-cli -p m80-jailer-harden
N=50 KIND=minimal KERNEL_KIND=stripped ./scripts/bench-cold-launch.sh --cold-isolation
```

Results:

| Cell | Snapshot | Result |
|---|---|---:|
| warm-cache reference | `crates/m80-firecracker/benches/snapshots/2026-05-13T16:34:08+00:00.json` | wall P50 1728 ms |
| cold-isolated baseline | `crates/m80-firecracker/benches/snapshots/2026-05-13T05:55:03+00:00.json` | wall P50 1837 ms |
| cold-isolated with artifact priming | `crates/m80-firecracker/benches/snapshots/2026-05-13T16:48:57+00:00.json` | wall P50 1846 ms, 1 failure |
| cold-isolated with harden plus pre-verify rootfs priming | `crates/m80-firecracker/benches/snapshots/2026-05-13T16:53:23+00:00.json` | wall P50 1848 ms |

The target was to reduce the cold-cold penalty from roughly 108 ms to 30 ms or
less. The measured cold-isolated runs stayed roughly 110-120 ms slower than the
warm-cache reference and slightly worse than the prior cold-isolated baseline.
On this host, `WILLNEED` is not sufficient to make cold launch behave like
warm-cache launch. Future work should start from a different mechanism, such as
preflight sentinel/cache placement or a dedicated artifact warming service, not
another inline fadvise call.

## Snapshot Files

`m80-jp6ik.34` adds `posix_fadvise(POSIX_FADV_WILLNEED)` for snapshot restore
inputs. `Sandbox::launch_from_snapshot` primes both host-visible snapshot
files before the snapshot directory is bind-mounted into the jail and before
Firecracker receives `/snapshot/vm.snap` and `/snapshot/mem.snap`.

## Baseline

- artifact: `crates/m80-firecracker/benches/snapshots/cold-restore-N50.json`
- cold restore P50: 93.945 ms
- warm restore P50: 55.316 ms
- cold-warm restore delta: 38.629 ms

## Fadvise Run

Command:

```sh
N=50 FILE_READ_SAMPLES=10 \
  IMAGE_BUILD_DIR_MINIMAL=/tmp/m80-build/minimal-jp6ik42 \
  SNAPSHOT_OUT=crates/m80-firecracker/benches/snapshots/cold-restore-fadvise-N50.json \
  bash scripts/bench-restore-cold.sh
```

- artifact: `crates/m80-firecracker/benches/snapshots/cold-restore-fadvise-N50.json`
- cold restore P50: 94.558 ms
- warm restore P50: 54.776 ms
- cold-warm restore delta: 39.782 ms
- `phase_restore_snapshot_prime` warm P50: 0.020 ms
- `phase_restore_snapshot_prime` cold P50: 0.317 ms

The fadvise call is cheap, but this run missed the close gate. Cold restore
P50 moved from 93.945 ms to 94.558 ms, a 0.613 ms regression rather than the
20-60 ms drop targeted by the bead.

The direct cold read of `mem.snap` was much larger than the restore-path tax,
and the fadvise run still shows that full-file read cost mostly outside the
restore-ready critical path. `phase_restore_load` moved from 3.252 ms cold P50
to 3.064 ms, while `phase_restore_probe_exec_channel` moved from 37.984 ms to
37.776 ms. Those small phase movements are not large enough to move end-to-end
cold restore latency.
