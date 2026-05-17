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

## Close-Gate Artifacts

Most generated files under `snapshots/` stay ignored. These named
measurement-shaped close artifacts are exceptions because their beads require
committed real-KVM evidence:

- `snapshot_template_restore_latency.json` (`m80-q420k.4.15`)

The Phase F composed-e2e artifacts are also kept trackable because
`m80-q420k.6.2` through `m80-q420k.6.5` close from the same quiet-host run:

- `snapshots/composed-e2e-restore-N10.json`
- `snapshots/composed-e2e-host-memory.json`
- `snapshots/composed-e2e-residue.json`

Do not commit diagnostic versions of those files from noisy-host or override
runs. Close-quality JSON records `substrate.allow_other_firecracker_vms=false`
and empty `substrate.preexisting_firecracker_processes` and
`substrate.post_run_firecracker_processes` arrays. It also records
`substrate.preflight_artifacts` with the resolved Firecracker, jailer, helper,
kernel, and rootfs identities measured by the run, plus a top-level
`git_commit`.

After writing a close-quality artifact, run the close guard before committing.
After committing the artifact, add `--require-committed` before closing the
bead, for example:

```sh
python3 scripts/verify-q420k-artifacts.py --only snapshot-template --require-committed
```

The `snapshot-template` selector also checks
`docs/perf/snapshot-template-restore.md`; the doc must include the bench stderr
line from the close-quality run that wrote the JSON artifact.

The Phase C Shared density artifact lives outside this benches directory at
`docs/perf/pmem-shared-density.md`; check it with `--only pmem-density`. That
selector also checks the executable close script
`scripts/smoke-pmem-shared.sh`, including its quiet-host fail-closed guard and
committed mode/content when `--require-committed` is present.

For final `m80-q420k` closure, run the verifier without `--only` and with
`--require-committed --require-closed-beads --require-parent-phases-closed`;
that checks the `.3.8` Shared density artifact, the snapshot-template artifact,
all three Phase F composed-e2e artifacts, the `docs/perf/composed-e2e.md`
receipt doc, the measurement beads' `verified: <artifact> @ <commit>` close
reasons, and the Phase 0 / A-F parent statuses. The verifier is a field,
git-state, close-reason, and tracker-state guard only. It does not replace the
real-KVM run or committed artifacts.

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
N=1000 SKIP_LOADED=1 ./scripts/bench-cold-launch.sh --cold-isolation
cp crates/m80-firecracker/benches/snapshots/latest.json \
   crates/m80-firecracker/benches/baseline.json
git add crates/m80-firecracker/benches/baseline.json
git commit -m "perf: snapshot new baseline (<reason>)"

# 2. After a candidate change:
N=200 SKIP_LOADED=1 ./scripts/bench-cold-launch.sh
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
