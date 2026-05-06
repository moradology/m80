# Cold Launch Phase Profile

Bead: `m80-w4vc`.

This profile is the residual true-cold launch lane after the storage pivot,
stripped-kernel, direct snapshot restore, persistent VM, and warm-pool work.
It is not an alternative to those paths; it answers what remains worth doing
when m80 must boot a brand-new VM.

Raw artifact: `docs/behaviors/lifecycle/cold-launch-phase-profile.json`.

## Method

The table below is a curated view over the existing `bench-cold-launch.sh`
snapshots from 2026-05-05. Each source snapshot was produced from per-run
CSV files, not the append-only historical CSVs. The curated JSON keeps image
kind, kernel kind, host load, success count, wallclock P50/P95/max, useful
phase sum, and per-phase P50/P95/max/count.

Loaded cells used the existing worst-case `stress-ng --cpu $(nproc)` shape.
That load shape is useful as a saturation probe, but it is harsher than a
normal CI host. The minimal loaded cells had 1/5 successes on stock and 0/5 on
stripped, so phase-bearing attempts are used only to classify the failure
shape, not as a stable loaded-latency target.

## Baseline

| cell | success | wallclock P50 | useful P50 | storage prep P50 | ready accept P50 | source |
|---|---:|---:|---:|---:|---:|---|
| minimal stock idle | 30/30 | 1517 ms | 1207 ms | 215.9 ms | 906.2 ms | `crates/m80-firecracker/benches/snapshots/2026-05-05T09:02:43+00:00.json` |
| minimal stock loaded | 1/5 | 1644 ms | 1318 ms | 254.6 ms | 955.0 ms | `crates/m80-firecracker/benches/snapshots/2026-05-05T09:03:20+00:00.json` |
| minimal stripped idle | 30/30 | 1417 ms | 1167 ms | 216.3 ms | 866.0 ms | `crates/m80-firecracker/benches/snapshots/2026-05-05T10:05:32+00:00.json` |
| minimal stripped loaded | 0/5 | none | 1242 ms | 300.2 ms | 866.6 ms | `crates/m80-firecracker/benches/snapshots/2026-05-05T10:06:03+00:00.json` |
| ubuntu stock idle | 30/30 | 3919 ms | 2534 ms | 1335.0 ms | 1108.0 ms | `crates/m80-firecracker/benches/snapshots/2026-05-05T10:12:04+00:00.json` |
| ubuntu stripped idle | 30/30 | 3818 ms | 2435 ms | 1337.0 ms | 1006.8 ms | `crates/m80-firecracker/benches/snapshots/2026-05-05T10:08:58+00:00.json` |

The minimal stripped idle cell is the best current cold-launch baseline: it is
boot-correct, has a 30/30 success rate, and is not inflated by Ubuntu rootfs
size or systemd. Its residual stack is:

| phase | P50 | P95 | max | classification |
|---|---:|---:|---:|---|
| `phase_12b_ready_accept` | 866.0 ms | 866.4 ms | 876.4 ms | guest kernel boot plus PID-1/guestd readiness |
| `phase_3_storage_prep` | 216.3 ms | 220.4 ms | 261.3 ms | host rootfs overlay image preparation |
| `phase_9_jailer_launch` | 25.4 ms | 25.5 ms | 25.5 ms | host Firecracker process launch via jailer |
| `phase_5b_cgroup_create` | 17.7 ms | 22.4 ms | 27.5 ms | host cgroup creation/attach |
| `phase_12a_instance_start` | 12.0 ms | 13.1 ms | 13.7 ms | Firecracker `InstanceStart` action |
| `phase_4_jailer_materialize` | 0.7 ms | 1.8 ms | 4.5 ms | jailer filesystem materialization |
| `phase_11_rest_puts` | 0.7 ms | 0.9 ms | 0.9 ms | Firecracker device/config REST calls |

## Angle Results

Guest-ready path reduction is the only plausible 100 ms plus cold-launch win
not already covered by storage work. The aggregate phase is about 866 ms P50
on the best minimal cell and about 1.0 to 1.1 s on Ubuntu, but the current
host-side phase trace cannot split kernel boot from guest PID-1 substeps.
The next bead should instrument guest-visible boot milestones before choosing
between ready-before-nonessential-setup, a smaller PID-1 path, delayed
workspace mount work, or further kernel profiling.

Pre-created overlay/rootfs artifacts should proceed through `m80-f2zc.10`.
The current storage prep target is `phase_3_storage_prep` below 80 ms P50 on
minimal idle, with P95 recorded. A broader pool of pre-created overlay images
is not justified until the reflink/template bead proves that template clone
still leaves a material residual or loaded P95 remains bad.

Firecracker setup and REST path work is not the next cold-launch priority.
The host phases that are clearly attributable to setup are below 50 ms each on
minimal stripped idle. Combining jailer launch, cgroup creation, materialize,
REST PUTs, and `InstanceStart` gives only about 56 ms P50, while guest-ready
and storage prep account for about 1082 ms P50. There is no immediate
implementation follow-up from this angle.

Direct/no-jailer launch is a dead-end product angle for now. A benchmark-only
direct path could at most remove the small jailed-host pieces, not the 866 ms
guest-ready phase or the 216 ms storage-prep phase. The bounded ceiling is
roughly 45 ms before security/resource tradeoffs, so no product-surface design
bead is warranted.

## Follow-Ups

Create one diagnostics implementation bead for guest PID-1 boot milestone
timings. Use that bead to split the 866 ms `phase_12b_ready_accept` chunk into
guest-visible substeps, then create a separate implementation bead only if one
substep can plausibly save at least 100 ms P50 or materially improve the
loaded-tail failure shape.

Keep `m80-f2zc.10` as the storage implementation bead. Its benchmark target is
`phase_3_storage_prep` below 80 ms P50 for minimal idle, with P95 and total
cold-launch impact recorded.

## Follow-Up Result: Guest Milestones and Template Clone

`m80-1f8.6` added guest boot milestones and the cold-launch bench now records
them as `guest_elapsed_*` and `guest_delta_*` phase rows. The detailed artifact
is `docs/behaviors/lifecycle/guest-boot-milestones.md`.

The follow-up data changes two recommendations:

- Guestd PID-1 userspace is not a 100 ms target. Minimal stock sends ready
  about 49.2 ms after process start; minimal stripped sends ready about
  54.6 ms after process start. Individual guest deltas are single-digit
  milliseconds.
- The `m80-f2zc.10` template path makes `Rootfs::prepare` about 13.8 ms P50,
  but total `phase_3_storage_prep` remains about 180.5 ms P50 because
  `phase_3a_manifest_verify` costs about 166.8 ms P50 per launch.

The next 100 ms plus candidates are therefore kernel/early guest boot
instrumentation and manifest-verification placement. Ready-before-finish,
lazy overlay mount, or broader overlay artifact pools are not supported by
this data. Manifest-verification placement is tracked by `m80-f2zc.11`.

`m80-f2zc.11` then moved the per-launch manifest sha256 recompute out of phase
3 and back to the existing `m80-preflight` trust boundary. Minimal stripped
idle N=30 improved from 1317 ms to 1117 ms wallclock P50; `phase_3_storage_prep`
fell from 180.5 ms to 13.4 ms P50. The ready phase did not move
(`phase_12b_ready_accept` stayed about 785.3 ms P50), so the next true-cold
100 ms plus target remains kernel/early guest boot.
