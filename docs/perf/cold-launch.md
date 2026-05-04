# Cold-launch latency: ubuntu vs minimal image

Bead m80-6a0q.6.

## Methodology

`scripts/bench-cold-launch.sh` runs `m80 launch -- /bin/echo bench-N`
against each image kind on each load level, captures wallclock per
launch, drops the first 2 launches per cell as warmup, computes
P50/P95/max.

Cells (4 total):

| kind    | load   | Notes                                           |
|---------|--------|-------------------------------------------------|
| ubuntu  | idle   | systemd as PID 1 (multi-user.target)            |
| ubuntu  | loaded | `stress-ng --cpu $(nproc)` running in parallel  |
| minimal | idle   | m80-guestd as PID 1 (no systemd)                |
| minimal | loaded | same stress-ng load                             |

Sample size: `N=30` per cell (configurable via env). Outputs:
- per-launch row in `crates/m80-firecracker/benches/cold-launch.csv`
- per-cell P50/P95/max printed to stdout

## What we're measuring

Wallclock from `m80 launch` invocation to exit. This includes:
- jailer + firecracker spawn
- the 12-phase preboot pipeline (machine config, drives, vsock, etc.)
- `InstanceStart` REST PUT
- guest kernel boot
- guestd init: vsock bind + ready handshake
- exec of `/bin/echo bench-N`
- vsock framing + response
- VM teardown

Note that this is end-to-end "launch a VM and run a command", not the
narrower "InstanceStart → ready" measurement that `phase_12b_ready_probe`
covers. v0.2 adds dedicated instrumentation for that finer split.

## Acceptance criteria (from the bead)

1. **Minimal P50 ≥ 50 ms below ubuntu P50** on idle host (the savings
   from skipping systemd boot must dominate the measurement noise).
2. **Minimal P95-vs-P50 spread ≤ ubuntu's** (small kernel + no systemd
   should reduce variance, not just the median).
3. **Minimal stress-ng cell**: P95 within 1.5× of idle P95 (the smaller
   userland should degrade more gracefully under load).

## Numbers

### Status: TBD — pending real-host run

Numbers will be filled in by running:

```bash
# Build both images first.
./scripts/smoke.sh                                              # ubuntu side
M80_RUN_ROOT=/var/lib/m80-run \
    cargo run --release -p m80-image-build -- run \
    --config m80-image-build-minimal.toml                       # minimal side
# Then bench:
./scripts/bench-cold-launch.sh
```

The `m80-image-build-minimal.toml` should declare:

```toml
[kernel]
version = "v1.15"
arch = "x86_64"

[rootfs]
size = "256MiB"
kind = "minimal"

[guestd]
binary = "/path/to/m80/target/x86_64-unknown-linux-musl/release/m80-guestd"

[output]
dir = "/tmp/m80-build/minimal"
```

(This depends on m80-6a0q.8 — the smoke variant — being able to drive
the minimal image kind. As of m80-6a0q.5 the m80-firecracker side reads
`manifest.image_kind` and dispatches boot args correctly.)

### Format once filled in

```
=== bench-cold-launch (N=30 per cell, warmup=2) ===

--- cell: ubuntu / idle ---
  ubuntu   idle    P50=____ms  P95=____ms  MAX=____ms  (n=30)
--- cell: ubuntu / loaded ---
  ubuntu   loaded  P50=____ms  P95=____ms  MAX=____ms  (n=30)
--- cell: minimal / idle ---
  minimal  idle    P50=____ms  P95=____ms  MAX=____ms  (n=30)
--- cell: minimal / loaded ---
  minimal  loaded  P50=____ms  P95=____ms  MAX=____ms  (n=30)
```

## Cross-reference

- Smolvm-comparison context: `smolvm-exploration/03-boot-path-and-readiness.md`
  — smolvm reports ~500 ms cold boot for their minimal image. m80's
  ubuntu number gives us the upper bound; m80's minimal gives us a
  direct apples-to-apples comparison.
- Phase-12b ready probe constants: `crates/m80-firecracker/src/launch.rs`
  — we are at `READY_POLL_INTERVAL = 10 ms` after m80-bgas.1, so the
  poll cadence is no longer the bottleneck.
