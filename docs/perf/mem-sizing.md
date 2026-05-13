# Guest Memory Sizing Sweep

Bead: `m80-jp6ik.44`

Date: 2026-05-13

Host/image: local privileged KVM host, minimal image at `/tmp/m80-build/minimal`, stock kernel, `EGRESS=none`.

Command:

```sh
SWEEP=mem_mib SWEEP_VALUES=256,512,1024,2048 KIND=minimal SKIP_LOADED=1 N=30 \
  ./scripts/bench-cold-launch.sh
```

Artifacts:

- `crates/m80-firecracker/benches/snapshots/mem-size-sweep.json`
- `crates/m80-firecracker/benches/sweep-mem_mib.csv`
- `crates/m80-firecracker/benches/sweep-mem_mib-phases.csv`
- `crates/m80-firecracker/benches/snapshots/mem-size-snapshot-<mib>.json`

## Results

| mem MiB | launch P50 ms | launch P95 ms | phase_12b_ready_accept P50 ms | phase_12b_ready_accept P95 ms | mem.snap bytes | C=8 guest RAM commit |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 256 | 1339 | 1341 | 1011.458 | 1018.512 | 268,435,456 | 2 GiB |
| 512 | 1339 | 1341 | 1012.732 | 1016.068 | 536,870,912 | 4 GiB |
| 1024 | 1339 | 1342 | 1012.986 | 1019.544 | 1,073,741,824 | 8 GiB |
| 2048 | 1339 | 1341 | 1049.614 | 1055.147 | 2,147,483,648 | 16 GiB |

## Interpretation

The 256, 512, and 1024 MiB cells are launch-equivalent in this minimal idle
probe: P50 wallclock is identical at 1339 ms, and `phase_12b_ready_accept`
varies by less than 2 ms across those three cells. This does not show a cold
launch win for 512 MiB over 1024 MiB on the current stock-kernel path.

The 2048 MiB cell adds about 36.6 ms to `phase_12b_ready_accept` P50 versus
1024 MiB, while coarse wallclock P50 remains unchanged. That points at a
small memory-size cost inside boot/ready wait, but not enough to affect the
end-to-end P50 at this sample size.

Snapshot size scales exactly with configured guest RAM. At C=8 density, moving
the first-line default from 1024 MiB to 512 MiB cuts guest RAM commitment from
8 GiB to 4 GiB and cuts full `mem.snap` size from 1 GiB to 512 MiB. 256 MiB has
the best commitment profile, but it is below the current safe workload envelope
for an agent-capable process wrapper. The data supports using 512 MiB as the
lower safe default. Bead `m80-jp6ik.29` applies that default-memory decision.
