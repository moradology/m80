# Bench artifacts

Files in this directory are produced by `scripts/bench-cold-launch.sh` and
consumed by `scripts/bench-summary.py`. Headers describe schema; values
inside are observational.

## Files

| File | What | Lifecycle |
|---|---|---|
| `cold-launch.csv` | append-only wallclock log: one row per bench attempt | accumulates across runs |
| `cold-launch-phases.csv` | append-only per-phase log; one row per `M80_PHASE` event | accumulates |
| `sweep-<var>.csv` | append-only sweep log; written when `SWEEP=<var>` | accumulates |
| `concurrent.csv` | append-only concurrent log; written when `CONCURRENT=N` | accumulates |
| `snapshots/<iso>.json` | per-run JSON snapshot computed from this run's CSVs only | one per run |
| `snapshots/latest.json` | symlink to the most recent snapshot | replaced each run |
| `baseline.json` | checked-in reference snapshot used by CI regression gate | replaced deliberately |

## Snapshot schema

```
{
  "version": 1,
  "data": {
    "wallclock": {
      "<kind>": {
        "<load>": {
          "p50": <ms>, "p75": <ms>, "p90": <ms>, "p95": <ms>,
          "p99": <ms>, "p999": <ms>, "max": <ms>,
          "count": <int>, "fail_count": <int>,
          "outliers": <int>,           // samples > 2σ from mean
          "p50_ci95": [<lo_ms>, <hi_ms>]  // bootstrap 95% CI for the median
        }
      }
    },
    "phases": {
      "<kind>": {
        "<load>": {
          "<phase>": {
            "p50_us": <us>, "p75_us": ..., "p999_us": ..., "max_us": <us>,
            "count": <int>,
            "histogram_us": [[low, high, count], ...]  // log-bucketed
          }
        }
      }
    },
    "useful_ms": { "<kind>": { "<load>": <int_p50_ms> } },
    "meta": { "generated_at": "<iso8601>" }
  }
}
```

## Workflow

```sh
# 1. Establish a baseline (run on the privileged-runner host once):
./scripts/bench-cold-launch.sh N=1000 SKIP_LOADED=1 --cold-isolation
cp crates/m80-firecracker/benches/snapshots/latest.json \
   crates/m80-firecracker/benches/baseline.json
git add crates/m80-firecracker/benches/baseline.json
git commit -m "perf: snapshot new baseline (<reason>)"

# 2. After a candidate change:
./scripts/bench-cold-launch.sh N=200 SKIP_LOADED=1
python3 scripts/bench-summary.py diff \
    crates/m80-firecracker/benches/baseline.json \
    crates/m80-firecracker/benches/snapshots/latest.json \
    --fail-on-regress 10

# 3. CI invokes the same diff with a configured threshold and fails the
#    job on regression.
```

## See also

- [`docs/perf/bench-harness.md`](../../../docs/perf/bench-harness.md) — full
  knob reference (N, WARMUP, SWEEP, CONCURRENT, TASKSET, CPU_GOVERNOR,
  --cold-isolation, --dry-run, PHASE_JSONL, M80_BIN).
- [`docs/perf/cold-launch.md`](../../../docs/perf/cold-launch.md) — the
  perf story (numbers + roadmap).
