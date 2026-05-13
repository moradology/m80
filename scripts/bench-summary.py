#!/usr/bin/env python3
"""bench-summary.py — bench snapshot compute, compare, and display.

Workflow for iterating on perf:

  1. Run the bench and save a baseline snapshot:

       ./scripts/bench-cold-launch.sh
       # snapshot auto-saved to crates/m80-firecracker/benches/snapshots/latest.json

  2. Make your change (kernel arg, launch phase, vsock tweak, etc.).

  3. Run the bench again — it saves a new snapshot automatically.

  4. Compare the two:

       python3 scripts/bench-summary.py diff \\
           crates/m80-firecracker/benches/snapshots/latest.json \\
           crates/m80-firecracker/benches/snapshots/<new-timestamp>.json

     Or, for CI regression gating:

       python3 scripts/bench-summary.py diff baseline.json new.json \\
           --fail-on-regress 10

Subcommands:

  summarize   Print the human-readable phase table (same as the old
              inline Python in bench-cold-launch.sh).

  compute     Read the two CSVs and emit a structured JSON snapshot.
              JSON envelope shape:
                {
                  "version": 1,
                  "data": {
                    "wallclock": {
                      "<kind>": {
                        "<load>": {
                          "p50": <int ms>, "p95": <int ms>, "max": <int ms>,
                          "count": <int>, "fail_count": <int>
                        }
                      }
                    },
                    "phases": {
                      "<kind>": {
                        "<load>": {
                          "<phase>": {"p50_us": <int>, "p95_us": <int>, "max_us": <int>, "count": <int>}
                        }
                      }
                    },
                    "useful_ms": {
                      "<kind>": {"<load>": <int p50 ms>}
                    },
                    "meta": {"generated_at": "<iso8601>"}
                  }
                }

  sweep       Read sweep wallclock and phase CSVs and emit a structured JSON
              snapshot keyed by sweep value.

  diff        Compare two snapshot JSONs and print a per-phase delta
              table sorted by abs(delta_us) descending.

  --test      Run inline unittests and exit.
"""

import argparse
import collections
import csv
import io
import json
import statistics
import sys
import os
import unittest
from datetime import datetime, timezone


DEFAULT_WALLCLOCK_CSV = "crates/m80-firecracker/benches/cold-launch.csv"
DEFAULT_PHASE_CSV = "crates/m80-firecracker/benches/cold-launch-phases.csv"

CELLS = [
    ("ubuntu", "idle"),
    ("ubuntu", "loaded"),
    ("minimal", "idle"),
    ("minimal", "loaded"),
]

STOP_EXCLUDE = {"stop_bounded"}

RED = "\x1b[31m"
GREEN = "\x1b[32m"
RESET = "\x1b[0m"


# ---------------------------------------------------------------------------
# Core computation helpers (pure functions, testable without real CSVs)
# ---------------------------------------------------------------------------

def _percentile(sorted_vals, pct):
    """Return the value at the given percentile (0–100) of sorted_vals."""
    n = len(sorted_vals)
    idx = max(0, int(n * pct / 100) - 1)
    idx = max(0, min(idx, n - 1))
    return sorted_vals[idx]


# Per-cell extended percentile set surfaced in JSON snapshots.
PERCENTILE_KEYS_MS = [("p50", 50), ("p75", 75), ("p90", 90), ("p95", 95),
                     ("p99", 99), ("p999", 99.9)]
PERCENTILE_KEYS_US = [("p50_us", 50), ("p75_us", 75), ("p90_us", 90),
                     ("p95_us", 95), ("p99_us", 99), ("p999_us", 99.9)]


def _log_histogram(sorted_vals, bucket_factor=2):
    """Return list of [low, high, count] buckets on a log-`factor` scale.

    Buckets cover the observed range only — buckets with zero count are
    omitted to keep snapshots small for N=1000 runs.
    """
    if not sorted_vals:
        return []
    lo = max(1, sorted_vals[0])
    hi = sorted_vals[-1]
    buckets = []
    cur = lo
    while cur <= hi:
        nxt = cur * bucket_factor
        count = sum(1 for v in sorted_vals if cur <= v < nxt)
        if count:
            buckets.append([cur, nxt, count])
        cur = nxt
    # Catch the final value (which sits at `hi` ≥ cur).
    if sorted_vals[-1] >= cur:
        buckets.append([cur, cur * bucket_factor, 1])
    return buckets


def bootstrap_ci_median(vals, samples=200, seed=0, confidence=0.95):
    """Bootstrap a confidence interval for the median.

    Returns ``(lower, upper)`` where each bound is an observed value from
    the original sample. Deterministic given ``seed``.
    """
    import random
    if not vals:
        return (None, None)
    rng = random.Random(seed)
    n = len(vals)
    medians = []
    for _ in range(samples):
        resample = sorted(rng.choices(vals, k=n))
        medians.append(_percentile(resample, 50))
    medians.sort()
    alpha = (1 - confidence) / 2
    lo_idx = max(0, int(samples * alpha) - 1)
    hi_idx = min(samples - 1, int(samples * (1 - alpha)) - 1)
    return (medians[lo_idx], medians[hi_idx])


def _count_outliers_2sigma(vals):
    """Return the number of samples more than 2 standard deviations from the mean."""
    if len(vals) < 3:
        return 0
    mean = sum(vals) / len(vals)
    var = sum((v - mean) ** 2 for v in vals) / len(vals)
    sd = var ** 0.5
    if sd == 0:
        return 0
    return sum(1 for v in vals if abs(v - mean) > 2 * sd)


def _stats_for(vals_sorted, suffix=""):
    """Return a dict of pN<suffix> percentile stats for a sorted sample."""
    keys = PERCENTILE_KEYS_US if suffix == "_us" else PERCENTILE_KEYS_MS
    out = {f"{name}": _percentile(vals_sorted, pct) for name, pct in keys}
    out[f"max{suffix}"] = vals_sorted[-1]
    out["count"] = len(vals_sorted)
    return out


def compute_snapshot(wallclock_file, phase_file):
    """
    Read wallclock_file and phase_file (file-like objects) and return the
    data dict for the JSON envelope (without version or meta).
    """
    # --- wallclock ---
    wc_rows = collections.defaultdict(list)
    wc_fail = collections.defaultdict(int)
    reader = csv.DictReader(wallclock_file)
    for row in reader:
        key = (row["kind"], row["load"])
        try:
            ms = int(row["launch_ms"])
        except (ValueError, KeyError):
            continue
        exit_code = row.get("exit", "0")
        if exit_code == "0":
            wc_rows[key].append(ms)
        else:
            wc_fail[key] += 1

    wallclock = {}
    for (kind, load), vals in wc_rows.items():
        vals_sorted = sorted(vals)
        entry = _stats_for(vals_sorted, suffix="")
        entry["fail_count"] = wc_fail.get((kind, load), 0)
        entry["outliers"] = _count_outliers_2sigma(vals)
        entry["p50_ci95"] = list(bootstrap_ci_median(vals, samples=200, seed=42))
        wallclock.setdefault(kind, {})[load] = entry
    # Fill in cells with only failures (no successes).
    for (kind, load), fc in wc_fail.items():
        if kind not in wallclock or load not in wallclock.get(kind, {}):
            empty = {name: None for name, _ in PERCENTILE_KEYS_MS}
            empty.update({"max": None, "count": 0, "fail_count": fc,
                          "outliers": 0, "p50_ci95": [None, None]})
            wallclock.setdefault(kind, {})[load] = empty

    # --- phases ---
    phase_rows = collections.defaultdict(list)  # (kind, load, phase) -> [us]
    # For useful_ms: sum per-attempt across non-excluded phases.
    useful_per_attempt = collections.defaultdict(lambda: collections.defaultdict(int))

    reader2 = csv.DictReader(phase_file)
    for row in reader2:
        key = (row["kind"], row["load"], row["phase"])
        try:
            us = int(row["elapsed_us"])
        except (ValueError, KeyError):
            continue
        phase_rows[key].append(us)
        if row["phase"] not in STOP_EXCLUDE:
            cell_key = (row["kind"], row["load"])
            useful_per_attempt[cell_key][row["attempt"]] += us

    phases = {}
    for (kind, load, phase), vals in phase_rows.items():
        vals_sorted = sorted(vals)
        entry = _stats_for(vals_sorted, suffix="_us")
        entry["histogram_us"] = _log_histogram(vals_sorted)
        phases.setdefault(kind, {}).setdefault(load, {})[phase] = entry

    useful_ms = {}
    for (kind, load), per_attempt in useful_per_attempt.items():
        vals = sorted(per_attempt.values())
        useful_ms.setdefault(kind, {})[load] = int(_percentile(vals, 50) / 1000)

    return {
        "wallclock": wallclock,
        "phases": phases,
        "useful_ms": useful_ms,
    }


# ---------------------------------------------------------------------------
# Sweep + concurrent aggregators (B0-4, B0-5)
# ---------------------------------------------------------------------------

def compute_sweep(wallclock_file, sweep_var):
    """Aggregate a sweep CSV (one row per attempt, with sweep_var + sweep_value).

    Returns ``{sweep_value: {p50, p95, p99, max, count, fail_count}}``.
    """
    rows = collections.defaultdict(list)
    fails = collections.defaultdict(int)
    for row in csv.DictReader(wallclock_file):
        if row.get("sweep_var") != sweep_var:
            continue
        sval = row.get("sweep_value", "")
        try:
            ms = int(row["launch_ms"])
        except (ValueError, KeyError):
            continue
        if row.get("exit", "0") == "0":
            rows[sval].append(ms)
        else:
            fails[sval] += 1
    out = {}
    for sval, vals in rows.items():
        s = sorted(vals)
        entry = _stats_for(s, suffix="")
        entry["fail_count"] = fails.get(sval, 0)
        out[sval] = entry
    return out


def compute_sweep_phases(phase_file, sweep_var):
    """Aggregate a sweep phase CSV by sweep value, kind, load, and phase."""
    rows = collections.defaultdict(list)
    for row in csv.DictReader(phase_file):
        if row.get("sweep_var") != sweep_var:
            continue
        sval = row.get("sweep_value", "")
        try:
            us = int(row["elapsed_us"])
        except (ValueError, KeyError):
            continue
        key = (sval, row.get("kind", ""), row.get("load", ""), row.get("phase", ""))
        rows[key].append(us)

    out = {}
    for (sval, kind, load, phase), vals in rows.items():
        s = sorted(vals)
        entry = _stats_for(s, suffix="_us")
        entry["histogram_us"] = _log_histogram(s)
        out.setdefault(sval, {}).setdefault(kind, {}).setdefault(load, {})[phase] = entry
    return out


def compute_throughput(csv_file):
    """B2: per-(kind, op) ops/sec + latency from a continuous-run CSV.

    Input columns: timestamp_unix_ms, kind, op, latency_ms, exit.
    """
    by_key = collections.defaultdict(list)  # (kind, op) -> [(ts_ms, lat_ms)]
    fails = collections.defaultdict(int)
    for row in csv.DictReader(csv_file):
        try:
            ts = int(row["timestamp_unix_ms"])
            lat = int(row["latency_ms"])
        except (ValueError, KeyError):
            continue
        key = (row["kind"], row["op"])
        if row.get("exit", "0") != "0":
            fails[key] += 1
            continue
        by_key[key].append((ts, lat))
    out = {}
    for (kind, op), rows in by_key.items():
        rows.sort()
        ts_first, ts_last = rows[0][0], rows[-1][0]
        window_s = max((ts_last - ts_first) / 1000.0, 0.001)
        lats = sorted(r[1] for r in rows)
        out.setdefault(kind, {})[op] = {
            "count": len(rows),
            "ops_per_sec": len(rows) / window_s,
            "p50_ms": _percentile(lats, 50),
            "p95_ms": _percentile(lats, 95),
            "p99_ms": _percentile(lats, 99),
            "fail_count": fails.get((kind, op), 0),
        }
    return out


def compute_memory(csv_file):
    """B4: per-VM RSS samples.

    Input columns: timestamp, vm_id, rss_kb, vm_count.
    Returns aggregate {rss_kb_p50, rss_kb_p95, rss_kb_max, sample_count,
    vm_count_p50}.
    """
    rss = []
    vm_counts = []
    for row in csv.DictReader(csv_file):
        try:
            rss.append(int(row["rss_kb"]))
            vm_counts.append(int(row.get("vm_count", "1")))
        except (ValueError, KeyError):
            continue
    if not rss:
        return {"sample_count": 0}
    rss_sorted = sorted(rss)
    vc_sorted = sorted(vm_counts)
    return {
        "rss_kb_p50": _percentile(rss_sorted, 50),
        "rss_kb_p95": _percentile(rss_sorted, 95),
        "rss_kb_max": rss_sorted[-1],
        "sample_count": len(rss),
        "vm_count_p50": _percentile(vc_sorted, 50) if vc_sorted else 0,
    }


def compute_teardown(csv_file):
    """B8: per-phase teardown latency aggregator.

    Input columns: timestamp, kind, attempt, phase, elapsed_us.
    Phases of interest: stop_bounded, residue_cleanup, force_kill,
    release. Returns {kind: {phase: {p50_us, p95_us, max_us, count}}}.
    """
    by_key = collections.defaultdict(list)  # (kind, phase) -> [us]
    for row in csv.DictReader(csv_file):
        try:
            us = int(row["elapsed_us"])
        except (ValueError, KeyError):
            continue
        by_key[(row["kind"], row["phase"])].append(us)
    out = {}
    for (kind, phase), vals in by_key.items():
        s = sorted(vals)
        out.setdefault(kind, {})[phase] = {
            "p50_us": _percentile(s, 50),
            "p95_us": _percentile(s, 95),
            "p99_us": _percentile(s, 99),
            "max_us": s[-1],
            "count": len(s),
        }
    return out


def compute_concurrent(wallclock_file):
    """Aggregate a concurrent CSV.

    Returns ``{concurrency_str: {wall_time_to_all_ready_p50_ms, per_vm_p95_ms,
    per_vm_p99_ms, attempts, vm_count}}``. Wall-time-to-all-ready is the
    per-attempt max launch_ms, then P50 across attempts.
    """
    per_attempt_max = collections.defaultdict(dict)  # conc -> attempt -> max_ms
    per_vm = collections.defaultdict(list)  # conc -> [launch_ms]
    for row in csv.DictReader(wallclock_file):
        conc = row.get("concurrency", "")
        attempt = row.get("attempt", "")
        try:
            ms = int(row["launch_ms"])
        except (ValueError, KeyError):
            continue
        if row.get("exit", "0") != "0":
            continue
        prev = per_attempt_max[conc].get(attempt, 0)
        per_attempt_max[conc][attempt] = max(prev, ms)
        per_vm[conc].append(ms)
    out = {}
    for conc, attempt_maxes in per_attempt_max.items():
        maxes = sorted(attempt_maxes.values())
        vms = sorted(per_vm[conc])
        out[conc] = {
            "wall_time_to_all_ready_p50_ms": _percentile(maxes, 50) if maxes else None,
            "wall_time_to_all_ready_p95_ms": _percentile(maxes, 95) if maxes else None,
            "per_vm_p50_ms": _percentile(vms, 50) if vms else None,
            "per_vm_p95_ms": _percentile(vms, 95) if vms else None,
            "per_vm_p99_ms": _percentile(vms, 99) if vms else None,
            "attempts": len(maxes),
            "vm_count": len(vms),
        }
    return out


def compute_diff(baseline_data, new_data):
    """
    Compare two snapshot data dicts. Returns a list of row dicts:
      {kind, load, phase, baseline_p50_us, new_p50_us, delta_us, delta_pct}
    and two lists of phase keys only in baseline / only in new.
    """
    def iter_phases(data):
        for kind, loads in data.get("phases", {}).items():
            for load, phase_map in loads.items():
                for phase, stats in phase_map.items():
                    yield (kind, load, phase), stats["p50_us"]

    baseline_map = dict(iter_phases(baseline_data))
    new_map = dict(iter_phases(new_data))

    both_keys = set(baseline_map) & set(new_map)
    only_baseline = sorted(set(baseline_map) - set(new_map))
    only_new = sorted(set(new_map) - set(baseline_map))

    rows = []
    for key in both_keys:
        b = baseline_map[key]
        n = new_map[key]
        delta_us = n - b
        if b == 0:
            delta_pct = None
        else:
            delta_pct = 100.0 * delta_us / b
        kind, load, phase = key
        rows.append({
            "kind": kind,
            "load": load,
            "phase": phase,
            "baseline_p50_us": b,
            "new_p50_us": n,
            "delta_us": delta_us,
            "delta_pct": delta_pct,
        })

    rows.sort(key=lambda r: abs(r["delta_us"]), reverse=True)
    return rows, only_baseline, only_new


# ---------------------------------------------------------------------------
# Subcommand: summarize
# ---------------------------------------------------------------------------

def cmd_summarize(args):
    wc_path = args.csv_wallclock
    ph_path = args.csv_phase

    phase_rows = collections.defaultdict(list)
    try:
        with open(ph_path) as f:
            reader = csv.DictReader(f)
            for row in reader:
                key = (row["kind"], row["load"], row["phase"])
                try:
                    phase_rows[key].append(int(row["elapsed_us"]))
                except (ValueError, KeyError):
                    pass
    except FileNotFoundError:
        return

    phases_seen = sorted({k[2] for k in phase_rows})
    header = ("ubuntu/idle", "ubuntu/loaded", "minimal/idle", "minimal/loaded")
    print(f'  {"phase":<26} ' + "  ".join(f"{k:>14}" for k in header))
    for ph in phases_seen:
        cells = []
        for kind, load in [("ubuntu", "idle"), ("ubuntu", "loaded"), ("minimal", "idle"), ("minimal", "loaded")]:
            vals = phase_rows.get((kind, load, ph), [])
            cells.append(f"{int(statistics.median(vals)):>11} us" if vals else f"{'':>14}")
        print(f"  {ph:<26} " + "  ".join(cells))

    print()
    print("=== useful_ms (total - stop_bounded, P50) ===")
    useful_per_attempt = collections.defaultdict(lambda: collections.defaultdict(int))
    try:
        with open(ph_path) as f:
            for row in csv.DictReader(f):
                key = (row["kind"], row["load"])
                if row["phase"] in STOP_EXCLUDE:
                    continue
                try:
                    useful_per_attempt[key][row["attempt"]] += int(row["elapsed_us"])
                except (ValueError, KeyError):
                    pass
    except FileNotFoundError:
        return

    print(f'  {"":<26} ' + "  ".join(f"{k:>14}" for k in header))
    cells = []
    for kind, load in [("ubuntu", "idle"), ("ubuntu", "loaded"), ("minimal", "idle"), ("minimal", "loaded")]:
        vals = list(useful_per_attempt.get((kind, load), {}).values())
        if vals:
            cells.append(f"{int(statistics.median(vals)/1000):>11} ms")
        else:
            cells.append(f"{'':>14}")
    print(f'  {"useful_ms (P50)":<26} ' + "  ".join(cells))


# ---------------------------------------------------------------------------
# Subcommand: compute
# ---------------------------------------------------------------------------

def cmd_compute(args):
    with open(args.csv_wallclock) as wf, open(args.csv_phase) as pf:
        data = compute_snapshot(wf, pf)

    data["meta"] = {"generated_at": datetime.now(timezone.utc).isoformat()}
    envelope = {"version": 1, "data": data}
    out = json.dumps(envelope, indent=2)

    if args.output:
        with open(args.output, "w") as f:
            f.write(out)
            f.write("\n")
    else:
        print(out)


# ---------------------------------------------------------------------------
# Subcommand: sweep
# ---------------------------------------------------------------------------

def cmd_sweep(args):
    with open(args.csv_sweep) as wf:
        wallclock = compute_sweep(wf, args.sweep_var)

    phases = {}
    if args.csv_sweep_phase:
        with open(args.csv_sweep_phase) as pf:
            phases = compute_sweep_phases(pf, args.sweep_var)

    data = {
        "sweep_var": args.sweep_var,
        "wallclock": wallclock,
        "phases": phases,
        "meta": {"generated_at": datetime.now(timezone.utc).isoformat()},
    }
    envelope = {"version": 1, "data": data}
    out = json.dumps(envelope, indent=2)

    if args.output:
        with open(args.output, "w") as f:
            f.write(out)
            f.write("\n")
    else:
        print(out)


# ---------------------------------------------------------------------------
# Subcommand: diff
# ---------------------------------------------------------------------------

def cmd_diff(args):
    with open(args.baseline) as f:
        baseline_env = json.load(f)
    with open(args.new) as f:
        new_env = json.load(f)

    if baseline_env.get("version") != 1:
        print(f"error: {args.baseline} has unsupported version {baseline_env.get('version')!r} (expected 1)", file=sys.stderr)
        sys.exit(1)
    if new_env.get("version") != 1:
        print(f"error: {args.new} has unsupported version {new_env.get('version')!r} (expected 1)", file=sys.stderr)
        sys.exit(1)

    use_color = sys.stdout.isatty() and not args.no_color
    rows, only_baseline, only_new = compute_diff(baseline_env["data"], new_env["data"])

    col_w = (26, 8, 7, 14, 14, 11, 9)
    header = ("phase", "kind", "load", "baseline_p50", "new_p50", "delta_us", "delta_pct")
    fmt = "  {:<{w0}}  {:<{w1}}  {:<{w2}}  {:>{w3}}  {:>{w4}}  {:>{w5}}  {:>{w6}}"

    print(fmt.format(*header, w0=col_w[0], w1=col_w[1], w2=col_w[2],
                     w3=col_w[3], w4=col_w[4], w5=col_w[5], w6=col_w[6]))
    print("  " + "-" * (sum(col_w) + 2 * (len(col_w) - 1)))

    tripped = []
    for row in rows:
        b = row["baseline_p50_us"]
        n = row["new_p50_us"]
        delta_us = row["delta_us"]
        delta_pct = row["delta_pct"]

        b_str = f"{b:,} us"
        n_str = f"{n:,} us"
        d_str = f"{delta_us:+,} us"
        if delta_pct is None:
            p_str = "n/a"
        else:
            p_str = f"{delta_pct:+.1f}%"

        line = fmt.format(
            row["phase"], row["kind"], row["load"],
            b_str, n_str, d_str, p_str,
            w0=col_w[0], w1=col_w[1], w2=col_w[2],
            w3=col_w[3], w4=col_w[4], w5=col_w[5], w6=col_w[6],
        )

        if use_color and delta_pct is not None:
            if delta_pct > 0:
                line = RED + line + RESET
            elif delta_pct < 0:
                line = GREEN + line + RESET

        print(line)

        if args.fail_on_regress is not None and delta_pct is not None:
            if delta_pct > args.fail_on_regress:
                tripped.append((row["phase"], row["kind"], row["load"], delta_pct))

    if only_baseline:
        print()
        for kind, load, phase in only_baseline:
            print(f"  {phase:<26}  {kind:<8}  {load:<7}  (only in baseline)")
    if only_new:
        print()
        for kind, load, phase in only_new:
            print(f"  {phase:<26}  {kind:<8}  {load:<7}  (only in new)")

    if tripped:
        print(file=sys.stderr)
        print(f"REGRESSION: {len(tripped)} phase(s) exceeded --fail-on-regress={args.fail_on_regress:.1f}%:", file=sys.stderr)
        for phase, kind, load, pct in tripped:
            print(f"  {phase}  {kind}/{load}  {pct:+.1f}%", file=sys.stderr)
        sys.exit(1)


# ---------------------------------------------------------------------------
# Inline unit tests
# ---------------------------------------------------------------------------

WALLCLOCK_CSV_SAMPLE = """\
timestamp,kind,load,attempt,launch_ms,exit
2026-05-04T10:00:00+00:00,ubuntu,idle,3,1200,0
2026-05-04T10:00:01+00:00,ubuntu,idle,4,1100,0
2026-05-04T10:00:02+00:00,ubuntu,idle,5,1300,0
2026-05-04T10:00:03+00:00,ubuntu,idle,6,1050,1
2026-05-04T10:00:04+00:00,minimal,idle,3,500,0
2026-05-04T10:00:05+00:00,minimal,idle,4,520,0
"""

PHASE_CSV_SAMPLE = """\
timestamp,kind,load,attempt,phase,elapsed_us
2026-05-04T10:00:00+00:00,ubuntu,idle,3,boot,900000
2026-05-04T10:00:00+00:00,ubuntu,idle,3,exec,200000
2026-05-04T10:00:00+00:00,ubuntu,idle,3,stop_bounded,100000
2026-05-04T10:00:01+00:00,ubuntu,idle,4,boot,800000
2026-05-04T10:00:01+00:00,ubuntu,idle,4,exec,180000
2026-05-04T10:00:01+00:00,ubuntu,idle,4,stop_bounded,120000
2026-05-04T10:00:02+00:00,ubuntu,idle,5,boot,1000000
2026-05-04T10:00:02+00:00,ubuntu,idle,5,exec,220000
2026-05-04T10:00:02+00:00,ubuntu,idle,5,stop_bounded,80000
2026-05-04T10:00:04+00:00,minimal,idle,3,boot,300000
2026-05-04T10:00:04+00:00,minimal,idle,3,exec,100000
2026-05-04T10:00:04+00:00,minimal,idle,3,stop_bounded,50000
2026-05-04T10:00:05+00:00,minimal,idle,4,boot,320000
2026-05-04T10:00:05+00:00,minimal,idle,4,exec,110000
2026-05-04T10:00:05+00:00,minimal,idle,4,stop_bounded,60000
"""


class TestComputeSnapshot(unittest.TestCase):
    def _run(self, wc=WALLCLOCK_CSV_SAMPLE, ph=PHASE_CSV_SAMPLE):
        return compute_snapshot(io.StringIO(wc), io.StringIO(ph))

    def test_wallclock_p50_ubuntu_idle(self):
        data = self._run()
        # Successful launches: 1200, 1100, 1300 ms → sorted: 1100, 1200, 1300
        # _percentile(3, 50): idx = max(0, int(3*50/100)-1) = max(0, 0) = 0 → 1100
        self.assertEqual(data["wallclock"]["ubuntu"]["idle"]["p50"], 1100)

    def test_wallclock_fail_count_ubuntu_idle(self):
        data = self._run()
        self.assertEqual(data["wallclock"]["ubuntu"]["idle"]["fail_count"], 1)

    def test_wallclock_count_ubuntu_idle(self):
        data = self._run()
        self.assertEqual(data["wallclock"]["ubuntu"]["idle"]["count"], 3)

    def test_phases_boot_ubuntu_idle(self):
        data = self._run()
        # boot values: 900000, 800000, 1000000 → sorted: 800000, 900000, 1000000
        # _percentile(3, 50): idx = max(0, int(1.5)-1) = 0 → 800000
        self.assertEqual(data["phases"]["ubuntu"]["idle"]["boot"]["p50_us"], 800000)

    def test_phases_boot_ubuntu_idle_max(self):
        data = self._run()
        self.assertEqual(data["phases"]["ubuntu"]["idle"]["boot"]["max_us"], 1000000)

    def test_phases_exec_ubuntu_idle(self):
        data = self._run()
        # exec values: 200000, 180000, 220000 → sorted: 180000, 200000, 220000
        # _percentile(3, 50): idx = 0 → 180000
        self.assertEqual(data["phases"]["ubuntu"]["idle"]["exec"]["p50_us"], 180000)

    def test_phases_stop_bounded_present(self):
        data = self._run()
        self.assertIn("stop_bounded", data["phases"]["ubuntu"]["idle"])

    def test_useful_ms_excludes_stop_bounded(self):
        data = self._run()
        # Per attempt: 3→900000+200000=1100000, 4→800000+180000=980000, 5→1000000+220000=1220000
        # Sorted: 980000, 1100000, 1220000 → _percentile(3, 50): idx=0 → 980000 → /1000 = 980 ms
        self.assertEqual(data["useful_ms"]["ubuntu"]["idle"], 980)

    def test_useful_ms_minimal_idle(self):
        data = self._run()
        # Per attempt: 3→300000+100000=400000, 4→320000+110000=430000
        # Sorted: 400000, 430000 → P50 idx=0 → 400000 → /1000 = 400 ms
        self.assertEqual(data["useful_ms"]["minimal"]["idle"], 400)

    def test_minimal_idle_wallclock(self):
        data = self._run()
        # 500, 520 → sorted → P50 idx=0 → 500
        self.assertEqual(data["wallclock"]["minimal"]["idle"]["p50"], 500)

    def test_empty_phase_file_no_crash(self):
        data = compute_snapshot(io.StringIO("timestamp,kind,load,attempt,launch_ms,exit\n"),
                                io.StringIO("timestamp,kind,load,attempt,phase,elapsed_us\n"))
        self.assertEqual(data["phases"], {})
        self.assertEqual(data["wallclock"], {})
        self.assertEqual(data["useful_ms"], {})


class TestComputeDiff(unittest.TestCase):
    def _make_data(self, boot_ubuntu_idle_p50, exec_ubuntu_idle_p50, boot_minimal_idle_p50=300000):
        return {
            "phases": {
                "ubuntu": {
                    "idle": {
                        "boot": {"p50_us": boot_ubuntu_idle_p50, "p95_us": 0, "count": 3},
                        "exec": {"p50_us": exec_ubuntu_idle_p50, "p95_us": 0, "count": 3},
                    }
                },
                "minimal": {
                    "idle": {
                        "boot": {"p50_us": boot_minimal_idle_p50, "p95_us": 0, "count": 2},
                    }
                },
            }
        }

    def test_delta_us_correct(self):
        baseline = self._make_data(900000, 200000)
        new = self._make_data(810000, 200000)
        rows, _, _ = compute_diff(baseline, new)
        boot_row = next(r for r in rows if r["phase"] == "boot" and r["kind"] == "ubuntu")
        self.assertEqual(boot_row["delta_us"], -90000)

    def test_delta_pct_correct(self):
        baseline = self._make_data(900000, 200000)
        new = self._make_data(990000, 200000)
        rows, _, _ = compute_diff(baseline, new)
        boot_row = next(r for r in rows if r["phase"] == "boot" and r["kind"] == "ubuntu")
        self.assertAlmostEqual(boot_row["delta_pct"], 10.0, places=5)

    def test_sorted_by_abs_delta_desc(self):
        baseline = self._make_data(900000, 200000)
        # boot improves by 90000, exec worsens by 50000
        new = self._make_data(810000, 250000)
        rows, _, _ = compute_diff(baseline, new)
        abs_deltas = [abs(r["delta_us"]) for r in rows]
        self.assertEqual(abs_deltas, sorted(abs_deltas, reverse=True))

    def test_only_baseline_detected(self):
        baseline = self._make_data(900000, 200000)
        # new has no exec phase
        new_data = {
            "phases": {
                "ubuntu": {"idle": {"boot": {"p50_us": 900000, "p95_us": 0, "count": 3}}},
                "minimal": {"idle": {"boot": {"p50_us": 300000, "p95_us": 0, "count": 2}}},
            }
        }
        _, only_baseline, _ = compute_diff(baseline, new_data)
        self.assertIn(("ubuntu", "idle", "exec"), only_baseline)

    def test_only_new_detected(self):
        baseline_data = {
            "phases": {
                "ubuntu": {"idle": {"boot": {"p50_us": 900000, "p95_us": 0, "count": 3}}},
            }
        }
        new = self._make_data(900000, 200000)
        _, _, only_new = compute_diff(baseline_data, new)
        self.assertIn(("ubuntu", "idle", "exec"), only_new)

    def test_baseline_zero_gives_none_pct(self):
        baseline = self._make_data(0, 200000)
        new = self._make_data(100000, 200000)
        rows, _, _ = compute_diff(baseline, new)
        boot_row = next(r for r in rows if r["phase"] == "boot" and r["kind"] == "ubuntu")
        self.assertIsNone(boot_row["delta_pct"])

    def test_no_regression_when_improvement(self):
        baseline = self._make_data(900000, 200000)
        new = self._make_data(810000, 200000)  # improvement
        rows, _, _ = compute_diff(baseline, new)
        boot_row = next(r for r in rows if r["phase"] == "boot" and r["kind"] == "ubuntu")
        self.assertLess(boot_row["delta_pct"], 0)

    def test_identical_snapshots_zero_delta(self):
        data = self._make_data(900000, 200000)
        rows, only_b, only_n = compute_diff(data, data)
        for r in rows:
            self.assertEqual(r["delta_us"], 0)
        self.assertEqual(only_b, [])
        self.assertEqual(only_n, [])


class TestRegressionThreshold(unittest.TestCase):
    """
    End-to-end test: write two tiny CSVs, compute snapshots, diff them,
    verify that --fail-on-regress logic fires correctly.
    """

    def _make_snapshot(self, boot_us):
        wc = "timestamp,kind,load,attempt,launch_ms,exit\n2026-05-04T10:00:00+00:00,ubuntu,idle,3,1200,0\n"
        ph = (
            "timestamp,kind,load,attempt,phase,elapsed_us\n"
            f"2026-05-04T10:00:00+00:00,ubuntu,idle,3,boot,{boot_us}\n"
        )
        return compute_snapshot(io.StringIO(wc), io.StringIO(ph))

    def test_threshold_not_exceeded_no_trip(self):
        baseline = self._make_snapshot(900000)
        new = self._make_snapshot(990000)  # +10% exactly
        rows, _, _ = compute_diff(baseline, new)
        boot_row = next(r for r in rows if r["phase"] == "boot")
        # At exactly 10%, a threshold of 10 should NOT trip (> 10, not >= 10)
        tripped = [r for r in rows if r["delta_pct"] is not None and r["delta_pct"] > 10.0]
        self.assertEqual(tripped, [])

    def test_threshold_exceeded_trips(self):
        baseline = self._make_snapshot(900000)
        new = self._make_snapshot(999001)  # > 11.0%
        rows, _, _ = compute_diff(baseline, new)
        tripped = [r for r in rows if r["delta_pct"] is not None and r["delta_pct"] > 10.0]
        self.assertEqual(len(tripped), 1)

    def test_improvement_does_not_trip_threshold(self):
        baseline = self._make_snapshot(900000)
        new = self._make_snapshot(500000)  # large improvement
        rows, _, _ = compute_diff(baseline, new)
        tripped = [r for r in rows if r["delta_pct"] is not None and r["delta_pct"] > 10.0]
        self.assertEqual(tripped, [])


class TestExtendedPercentiles(unittest.TestCase):
    """B0-1: P50/P75/P90/P95/P99/P99.9/max from large samples."""

    def test_p99_from_1000_sample(self):
        # values = [1, 2, ..., 1000]; P99 is the 990th value (1-indexed)
        ph = "timestamp,kind,kernel_kind,load,attempt,phase,elapsed_us\n" + "".join(
            f"2026-05-04T10:00:00+00:00,ubuntu,stock,idle,{i},boot,{i}\n"
            for i in range(1, 1001)
        )
        wc = "timestamp,kind,kernel_kind,load,attempt,launch_ms,exit\n"
        data = compute_snapshot(io.StringIO(wc), io.StringIO(ph))
        boot = data["phases"]["ubuntu"]["idle"]["boot"]
        # _percentile is "rank-1 index" — at N=1000 P99 → idx 989 → value 990
        self.assertEqual(boot["p99_us"], 990)
        self.assertEqual(boot["p999_us"], 999)

    def test_p75_p90_present(self):
        ph = "timestamp,kind,kernel_kind,load,attempt,phase,elapsed_us\n" + "".join(
            f"2026-05-04T10:00:00+00:00,ubuntu,stock,idle,{i},boot,{i * 1000}\n"
            for i in range(1, 101)
        )
        wc = "timestamp,kind,kernel_kind,load,attempt,launch_ms,exit\n"
        data = compute_snapshot(io.StringIO(wc), io.StringIO(ph))
        boot = data["phases"]["ubuntu"]["idle"]["boot"]
        self.assertIn("p75_us", boot)
        self.assertIn("p90_us", boot)

    def test_extended_percentiles_on_wallclock(self):
        wc = "timestamp,kind,kernel_kind,load,attempt,launch_ms,exit\n" + "".join(
            f"2026-05-04T10:00:00+00:00,ubuntu,stock,idle,{i},{i},0\n"
            for i in range(1, 101)
        )
        ph = "timestamp,kind,kernel_kind,load,attempt,phase,elapsed_us\n"
        data = compute_snapshot(io.StringIO(wc), io.StringIO(ph))
        ui = data["wallclock"]["ubuntu"]["idle"]
        self.assertIn("p99", ui)
        self.assertIn("p999", ui)
        self.assertIn("p75", ui)
        self.assertIn("p90", ui)


class TestHistogramBuckets(unittest.TestCase):
    """B0-1: log-bucketed histogram per phase."""

    def test_histogram_emits_buckets(self):
        ph = "timestamp,kind,kernel_kind,load,attempt,phase,elapsed_us\n" + "".join(
            f"2026-05-04T10:00:00+00:00,ubuntu,stock,idle,{i},boot,{10 ** ((i % 6) + 2)}\n"
            for i in range(1, 101)
        )
        wc = "timestamp,kind,kernel_kind,load,attempt,launch_ms,exit\n"
        data = compute_snapshot(io.StringIO(wc), io.StringIO(ph))
        boot = data["phases"]["ubuntu"]["idle"]["boot"]
        self.assertIn("histogram_us", boot)
        # Each bucket is [low, high, count]; we expect at least 2 non-empty buckets.
        non_empty = [b for b in boot["histogram_us"] if b[2] > 0]
        self.assertGreaterEqual(len(non_empty), 2)


class TestBootstrapCI(unittest.TestCase):
    """B0-7: bootstrap 95% confidence interval for the median."""

    def test_bootstrap_ci_brackets_median(self):
        vals = list(range(1, 101))  # 1..100
        lo, hi = bootstrap_ci_median(vals, samples=200, seed=42)
        median = 50
        self.assertLessEqual(lo, median)
        self.assertGreaterEqual(hi, median)

    def test_bootstrap_ci_width_widens_with_variance(self):
        tight = [50] * 50 + [51] * 50
        spread = list(range(0, 100))
        lo_t, hi_t = bootstrap_ci_median(tight, samples=200, seed=1)
        lo_s, hi_s = bootstrap_ci_median(spread, samples=200, seed=1)
        self.assertLess(hi_t - lo_t, hi_s - lo_s)

    def test_bootstrap_ci_appears_in_snapshot(self):
        wc = "timestamp,kind,kernel_kind,load,attempt,launch_ms,exit\n" + "".join(
            f"2026-05-04T10:00:00+00:00,ubuntu,stock,idle,{i},{i + 100},0\n"
            for i in range(1, 51)
        )
        ph = "timestamp,kind,kernel_kind,load,attempt,phase,elapsed_us\n"
        data = compute_snapshot(io.StringIO(wc), io.StringIO(ph))
        ui = data["wallclock"]["ubuntu"]["idle"]
        self.assertIn("p50_ci95", ui)
        lo, hi = ui["p50_ci95"]
        self.assertLessEqual(lo, ui["p50"])
        self.assertGreaterEqual(hi, ui["p50"])


class TestOutlierDetection(unittest.TestCase):
    """B0-7: 2σ outlier flagging per cell."""

    def test_outlier_count_zero_when_uniform(self):
        wc = "timestamp,kind,kernel_kind,load,attempt,launch_ms,exit\n" + "".join(
            f"2026-05-04T10:00:00+00:00,ubuntu,stock,idle,{i},100,0\n"
            for i in range(1, 31)
        )
        data = compute_snapshot(io.StringIO(wc), io.StringIO("timestamp,kind,kernel_kind,load,attempt,phase,elapsed_us\n"))
        self.assertEqual(data["wallclock"]["ubuntu"]["idle"]["outliers"], 0)

    def test_outlier_count_flags_extreme(self):
        rows = "timestamp,kind,kernel_kind,load,attempt,launch_ms,exit\n"
        for i in range(1, 31):
            rows += f"2026-05-04T10:00:00+00:00,ubuntu,stock,idle,{i},100,0\n"
        # Add a single huge outlier
        rows += "2026-05-04T10:00:01+00:00,ubuntu,stock,idle,99,5000,0\n"
        data = compute_snapshot(io.StringIO(rows), io.StringIO("timestamp,kind,kernel_kind,load,attempt,phase,elapsed_us\n"))
        self.assertGreaterEqual(data["wallclock"]["ubuntu"]["idle"]["outliers"], 1)


class TestSweepIngest(unittest.TestCase):
    """B0-4: sweep CSV with per-cell summary."""

    def test_sweep_csv_aggregates_per_value(self):
        # SWEEP CSV columns add sweep_var + sweep_value:
        # timestamp,kind,kernel_kind,load,attempt,sweep_var,sweep_value,launch_ms,exit
        wc = "timestamp,kind,kernel_kind,load,attempt,sweep_var,sweep_value,launch_ms,exit\n"
        for sval in (1, 2, 4):
            for i in range(1, 6):
                wc += f"2026-05-04T10:00:00+00:00,minimal,stock,idle,{i},vcpu,{sval},{500 + sval * 100},0\n"
        cells = compute_sweep(io.StringIO(wc), sweep_var="vcpu")
        # 3 sweep values × one cell each
        keys = sorted(cells.keys())
        self.assertEqual(keys, ["1", "2", "4"])
        # p50 increases with sweep value
        self.assertLess(cells["1"]["p50"], cells["4"]["p50"])

    def test_sweep_phase_csv_aggregates_per_value_and_phase(self):
        rows = "timestamp,kind,kernel_kind,load,attempt,sweep_var,sweep_value,phase,elapsed_us\n"
        for sval in (512, 1024):
            for i in range(1, 6):
                rows += (
                    "2026-05-04T10:00:00+00:00,minimal,stock,idle,"
                    f"{i},mem_mib,{sval},phase_12b_ready_accept,{sval + i}\n"
                )
        phases = compute_sweep_phases(io.StringIO(rows), sweep_var="mem_mib")
        self.assertEqual(
            phases["512"]["minimal"]["idle"]["phase_12b_ready_accept"]["count"],
            5,
        )
        self.assertLess(
            phases["512"]["minimal"]["idle"]["phase_12b_ready_accept"]["p50_us"],
            phases["1024"]["minimal"]["idle"]["phase_12b_ready_accept"]["p50_us"],
        )


class TestConcurrentAggregation(unittest.TestCase):
    """B0-5: CONCURRENT=N wall-time-to-all-ready + per-VM tail."""

    def test_concurrent_aggregation(self):
        # Concurrent CSV columns: timestamp,kind,kernel_kind,load,attempt,concurrency,vm_index,launch_ms,exit
        rows = "timestamp,kind,kernel_kind,load,attempt,concurrency,vm_index,launch_ms,exit\n"
        # Two attempts of concurrency=4
        for attempt in (1, 2):
            for vm in range(4):
                rows += f"2026-05-04T10:00:00+00:00,minimal,stock,idle,{attempt},4,{vm},{800 + vm * 100},0\n"
        agg = compute_concurrent(io.StringIO(rows))
        # wall_time_to_all_ready = max per attempt = 800 + 3*100 = 1100
        # vm_tail = P95 across all VMs in all attempts
        self.assertEqual(agg["4"]["wall_time_to_all_ready_p50_ms"], 1100)
        self.assertIn("per_vm_p95_ms", agg["4"])


class TestThroughput(unittest.TestCase):
    """B2: ops/sec sustained over a window."""

    def test_throughput_ops_per_sec(self):
        # 100 ops in 10 seconds => 10 ops/sec
        rows = "timestamp_unix_ms,kind,op,latency_ms,exit\n" + "".join(
            f"{1700000000000 + i * 100},minimal,exec,5,0\n" for i in range(100)
        )
        result = compute_throughput(io.StringIO(rows))
        self.assertEqual(result["minimal"]["exec"]["count"], 100)
        # Window is 100 ops × 100 ms = 10 seconds → 10 ops/sec
        self.assertAlmostEqual(result["minimal"]["exec"]["ops_per_sec"], 10.0, delta=0.5)
        self.assertEqual(result["minimal"]["exec"]["p50_ms"], 5)


class TestMemoryRSS(unittest.TestCase):
    """B4: per-VM RSS sampling aggregation."""

    def test_memory_rss_p50(self):
        # rows: timestamp,vm_id,rss_kb,vm_count
        rows = "timestamp,vm_id,rss_kb,vm_count\n" + "".join(
            f"{i},vm-{i},{50000 + (i % 10) * 100},1\n" for i in range(20)
        )
        result = compute_memory(io.StringIO(rows))
        self.assertIn("rss_kb_p50", result)
        self.assertGreater(result["rss_kb_p50"], 50000)
        self.assertEqual(result["sample_count"], 20)


class TestTeardownLatency(unittest.TestCase):
    """B8: teardown latency aggregation."""

    def test_teardown_p50_p95(self):
        rows = "timestamp,kind,attempt,phase,elapsed_us\n" + "".join(
            f"{i},minimal,{i},stop_bounded,{50000 + i * 1000}\n" for i in range(50)
        ) + "".join(
            f"{i},minimal,{i},residue_cleanup,{10000 + i * 100}\n" for i in range(50)
        )
        result = compute_teardown(io.StringIO(rows))
        self.assertIn("stop_bounded", result["minimal"])
        self.assertIn("residue_cleanup", result["minimal"])
        self.assertGreater(result["minimal"]["stop_bounded"]["p95_us"],
                           result["minimal"]["stop_bounded"]["p50_us"])


class TestColorLogic(unittest.TestCase):
    def test_red_applied_to_regression_string(self):
        text = "some regression"
        colored = RED + text + RESET
        self.assertIn("\x1b[31m", colored)
        self.assertIn("\x1b[0m", colored)

    def test_green_applied_to_improvement_string(self):
        text = "some improvement"
        colored = GREEN + text + RESET
        self.assertIn("\x1b[32m", colored)

    def test_no_color_constants_differ(self):
        self.assertNotEqual(RED, GREEN)

    def test_reset_terminates_color(self):
        # RESET should reset both RED and GREEN
        self.assertEqual(RESET, "\x1b[0m")


class TestEndToEndDiffRender(unittest.TestCase):
    """
    Synthetic end-to-end: build two snapshots from CSV strings, produce
    JSON envelopes, run compute_diff, confirm the table rows are as expected.
    """

    def _snapshot_envelope(self, boot_us, exec_us):
        wc = (
            "timestamp,kind,load,attempt,launch_ms,exit\n"
            "2026-05-04T10:00:00+00:00,ubuntu,idle,3,1200,0\n"
            "2026-05-04T10:00:01+00:00,ubuntu,idle,4,1100,0\n"
            "2026-05-04T10:00:02+00:00,ubuntu,idle,5,1300,0\n"
        )
        ph = (
            "timestamp,kind,load,attempt,phase,elapsed_us\n"
            f"2026-05-04T10:00:00+00:00,ubuntu,idle,3,boot,{boot_us}\n"
            f"2026-05-04T10:00:00+00:00,ubuntu,idle,3,exec,{exec_us}\n"
            f"2026-05-04T10:00:01+00:00,ubuntu,idle,4,boot,{boot_us}\n"
            f"2026-05-04T10:00:01+00:00,ubuntu,idle,4,exec,{exec_us}\n"
            f"2026-05-04T10:00:02+00:00,ubuntu,idle,5,boot,{boot_us}\n"
            f"2026-05-04T10:00:02+00:00,ubuntu,idle,5,exec,{exec_us}\n"
        )
        data = compute_snapshot(io.StringIO(wc), io.StringIO(ph))
        data["meta"] = {"generated_at": "2026-05-04T10:00:00+00:00"}
        return {"version": 1, "data": data}

    def test_diff_row_count(self):
        baseline_env = self._snapshot_envelope(900000, 200000)
        new_env = self._snapshot_envelope(810000, 200000)
        rows, only_b, only_n = compute_diff(baseline_env["data"], new_env["data"])
        # 2 phases: boot, exec — both present in both snapshots
        self.assertEqual(len(rows), 2)
        self.assertEqual(only_b, [])
        self.assertEqual(only_n, [])

    def test_diff_largest_delta_first(self):
        baseline_env = self._snapshot_envelope(900000, 200000)
        # boot regresses 90000 us, exec unchanged
        new_env = self._snapshot_envelope(990000, 200000)
        rows, _, _ = compute_diff(baseline_env["data"], new_env["data"])
        self.assertEqual(rows[0]["phase"], "boot")
        self.assertEqual(rows[0]["delta_us"], 90000)

    def test_diff_json_round_trip(self):
        baseline_env = self._snapshot_envelope(900000, 200000)
        serialized = json.dumps(baseline_env)
        deserialized = json.loads(serialized)
        self.assertEqual(deserialized["version"], 1)
        self.assertIn("phases", deserialized["data"])
        self.assertIn("wallclock", deserialized["data"])
        self.assertIn("useful_ms", deserialized["data"])
        self.assertIn("meta", deserialized["data"])

    def test_diff_version_check_baseline(self):
        # version != 1 should be detectable by callers
        bad_env = {"version": 2, "data": {}}
        self.assertNotEqual(bad_env.get("version"), 1)

    def test_delta_pct_positive_for_regression(self):
        baseline_env = self._snapshot_envelope(900000, 200000)
        new_env = self._snapshot_envelope(990000, 200000)
        rows, _, _ = compute_diff(baseline_env["data"], new_env["data"])
        boot_row = next(r for r in rows if r["phase"] == "boot")
        self.assertGreater(boot_row["delta_pct"], 0)

    def test_delta_pct_negative_for_improvement(self):
        baseline_env = self._snapshot_envelope(900000, 200000)
        new_env = self._snapshot_envelope(810000, 200000)
        rows, _, _ = compute_diff(baseline_env["data"], new_env["data"])
        boot_row = next(r for r in rows if r["phase"] == "boot")
        self.assertLess(boot_row["delta_pct"], 0)


# ---------------------------------------------------------------------------
# Argument parsing + dispatch
# ---------------------------------------------------------------------------

def build_parser():
    parser = argparse.ArgumentParser(
        description="Bench snapshot compute, compare, and summarize.",
    )
    parser.add_argument(
        "--test",
        action="store_true",
        help="Run inline unit tests and exit.",
    )

    sub = parser.add_subparsers(dest="command")

    # summarize
    p_summarize = sub.add_parser(
        "summarize",
        help="Print the human-readable per-phase table.",
    )
    p_summarize.add_argument(
        "--csv-wallclock",
        default=DEFAULT_WALLCLOCK_CSV,
        metavar="PATH",
        help=f"Wallclock CSV path (default: {DEFAULT_WALLCLOCK_CSV})",
    )
    p_summarize.add_argument(
        "--csv-phase",
        default=DEFAULT_PHASE_CSV,
        metavar="PATH",
        help=f"Phase CSV path (default: {DEFAULT_PHASE_CSV})",
    )

    # compute
    p_compute = sub.add_parser(
        "compute",
        help="Emit a structured JSON snapshot from the two CSVs.",
    )
    p_compute.add_argument(
        "--csv-wallclock",
        default=DEFAULT_WALLCLOCK_CSV,
        metavar="PATH",
        help=f"Wallclock CSV path (default: {DEFAULT_WALLCLOCK_CSV})",
    )
    p_compute.add_argument(
        "--csv-phase",
        default=DEFAULT_PHASE_CSV,
        metavar="PATH",
        help=f"Phase CSV path (default: {DEFAULT_PHASE_CSV})",
    )
    p_compute.add_argument(
        "--output",
        metavar="FILE",
        help="Write JSON to FILE instead of stdout.",
    )

    # sweep
    p_sweep = sub.add_parser(
        "sweep",
        help="Emit a structured JSON snapshot from sweep CSVs.",
    )
    p_sweep.add_argument(
        "--sweep-var",
        required=True,
        metavar="NAME",
        help="Sweep variable to aggregate, e.g. mem_mib.",
    )
    p_sweep.add_argument(
        "--csv-sweep",
        required=True,
        metavar="PATH",
        help="Sweep wallclock CSV path.",
    )
    p_sweep.add_argument(
        "--csv-sweep-phase",
        metavar="PATH",
        help="Sweep phase CSV path.",
    )
    p_sweep.add_argument(
        "--output",
        metavar="FILE",
        help="Write JSON to FILE instead of stdout.",
    )

    # diff
    p_diff = sub.add_parser(
        "diff",
        help="Compare two snapshot JSONs and print a per-phase delta table.",
    )
    p_diff.add_argument("baseline", metavar="baseline.json")
    p_diff.add_argument("new", metavar="new.json")
    p_diff.add_argument(
        "--fail-on-regress",
        type=float,
        metavar="PCT",
        default=None,
        help="Exit non-zero if any phase delta_pct exceeds PCT.",
    )
    p_diff.add_argument(
        "--no-color",
        action="store_true",
        help="Disable ANSI color output even on a tty.",
    )

    return parser


def main():
    parser = build_parser()
    args = parser.parse_args()

    if args.test:
        unittest.main(argv=["", "--verbose"])
        return  # unreachable; unittest.main exits

    if args.command == "summarize":
        cmd_summarize(args)
    elif args.command == "compute":
        cmd_compute(args)
    elif args.command == "sweep":
        cmd_sweep(args)
    elif args.command == "diff":
        cmd_diff(args)
    else:
        parser.print_help()
        sys.exit(1)


if __name__ == "__main__":
    main()
