#!/usr/bin/env bash
# Extended density sweep for m80-jp6ik.42.
#
# Runs concurrent cold-launch cells for no-egress and outbound modes, then
# writes a dedicated density CSV and JSON summary.

set -euo pipefail

cd "$(dirname "$0")/.."

N="${N:-20}"
WARMUP="${WARMUP:-2}"
KIND="${KIND:-minimal}"
KERNEL_KIND="${KERNEL_KIND:-${M80_KERNEL_KIND:-stripped}}"
M80_BIN="${M80_BIN:-./target/release/m80}"
NO_EGRESS_LADDER="${NO_EGRESS_LADDER:-1,2,4,8,16,32,48,64}"
OUTBOUND_LADDER="${OUTBOUND_LADDER:-1,2,4,8,16,32}"
OUTDIR="crates/m80-firecracker/benches"
DENSITY_CSV="$OUTDIR/density-extended.csv"
DENSITY_JSON="$OUTDIR/snapshots/density-extended.json"
DRY_RUN=0

usage() {
    cat <<'EOF'
bench-density-extended.sh — m80-jp6ik.42 density sweep

Usage:
  scripts/bench-density-extended.sh [--dry-run]

Env vars:
  N=20
  WARMUP=2
  KIND=minimal
  KERNEL_KIND=stripped
  M80_BIN=./target/release/m80
  NO_EGRESS_LADDER=1,2,4,8,16,32,48,64
  OUTBOUND_LADDER=1,2,4,8,16,32

Outputs:
  crates/m80-firecracker/benches/density-extended.csv
  crates/m80-firecracker/benches/snapshots/density-extended.json
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run) DRY_RUN=1; shift ;;
        --help|-h) usage; exit 0 ;;
        *) echo "unknown flag: $1" >&2; usage >&2; exit 2 ;;
    esac
done

plan() {
    echo "=== bench-density-extended plan ==="
    echo "  N=$N  WARMUP=$WARMUP  KIND=$KIND  KERNEL_KIND=$KERNEL_KIND"
    echo "  no-egress ladder: $NO_EGRESS_LADDER"
    echo "  outbound ladder:  $OUTBOUND_LADDER"
    echo "  output: $DENSITY_CSV"
    echo "  snapshot: $DENSITY_JSON"
}

append_concurrent_rows() {
    local egress="$1"
    local source_csv="$2"
    python3 - "$egress" "$source_csv" "$DENSITY_CSV" <<'PY'
import csv
import sys

egress, source, dest = sys.argv[1:]
with open(source, newline="") as f, open(dest, "a", newline="") as out:
    reader = csv.DictReader(f)
    writer = csv.writer(out)
    for row in reader:
        writer.writerow([
            row["timestamp"],
            egress,
            row["kind"],
            row["kernel_kind"],
            row["load"],
            row["attempt"],
            row["concurrency"],
            row["vm_index"],
            row["launch_ms"],
            row["exit"],
        ])
PY
}

write_summary_json() {
    python3 - "$DENSITY_CSV" "$DENSITY_JSON" <<'PY'
import csv
import json
import math
import statistics
import sys
from collections import defaultdict
from datetime import datetime, timezone

source, dest = sys.argv[1:]

def percentile(values, pct):
    if not values:
        return None
    values = sorted(values)
    idx = math.ceil((pct / 100) * len(values)) - 1
    idx = max(0, min(idx, len(values) - 1))
    return values[idx]

attempt_max = defaultdict(dict)
per_vm = defaultdict(list)
failures = defaultdict(int)
with open(source, newline="") as f:
    for row in csv.DictReader(f):
        key = (row["egress"], row["kind"], row["kernel_kind"], row["load"], row["concurrency"])
        try:
            launch_ms = int(row["launch_ms"])
        except ValueError:
            continue
        if row["exit"] != "0":
            failures[key] += 1
            continue
        attempt = row["attempt"]
        attempt_max[key][attempt] = max(attempt_max[key].get(attempt, 0), launch_ms)
        per_vm[key].append(launch_ms)

cells = []
for key in sorted(set(attempt_max) | set(failures)):
    egress, kind, kernel_kind, load, concurrency = key
    maxes = sorted(attempt_max[key].values())
    vms = sorted(per_vm[key])
    cells.append({
        "egress": egress,
        "kind": kind,
        "kernel_kind": kernel_kind,
        "load": load,
        "concurrency": int(concurrency),
        "attempts": len(maxes),
        "vm_count": len(vms),
        "fail_count": failures.get(key, 0),
        "wall_time_to_all_ready_p50_ms": percentile(maxes, 50),
        "wall_time_to_all_ready_p95_ms": percentile(maxes, 95),
        "wall_time_to_all_ready_p99_ms": percentile(maxes, 99),
        "wall_time_to_all_ready_max_ms": max(maxes) if maxes else None,
        "per_vm_p50_ms": percentile(vms, 50),
        "per_vm_p95_ms": percentile(vms, 95),
        "per_vm_p99_ms": percentile(vms, 99),
        "per_vm_max_ms": max(vms) if vms else None,
    })

payload = {
    "schema_version": 1,
    "generated_at": datetime.now(timezone.utc).isoformat(),
    "source_csv": source,
    "cells": cells,
}
with open(dest, "w") as f:
    json.dump(payload, f, indent=2, sort_keys=True)
    f.write("\n")

for cell in cells:
    print(
        "{egress:8s} C={concurrency:<2d} attempts={attempts:<2d} "
        "wall_p50={wall_time_to_all_ready_p50_ms}ms "
        "wall_p95={wall_time_to_all_ready_p95_ms}ms "
        "per_vm_p99={per_vm_p99_ms}ms failures={fail_count}".format(**cell)
    )
PY
}

plan
if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "(dry-run) skipping launches."
    exit 0
fi

mkdir -p "$OUTDIR" "$(dirname "$DENSITY_JSON")"
echo "timestamp,egress,kind,kernel_kind,load,attempt,concurrency,vm_index,launch_ms,exit" > "$DENSITY_CSV"

for spec in "none:$NO_EGRESS_LADDER" "outbound:$OUTBOUND_LADDER"; do
    egress="${spec%%:*}"
    ladder="${spec#*:}"
    IFS=',' read -r -a steps <<<"$ladder"
    for c in "${steps[@]}"; do
        echo "--- density cell: egress=$egress concurrency=$c ---"
        rm -f "$OUTDIR/concurrent.csv"
        EGRESS="$egress" \
            CONCURRENT="$c" \
            N="$N" \
            WARMUP="$WARMUP" \
            KIND="$KIND" \
            KERNEL_KIND="$KERNEL_KIND" \
            SKIP_LOADED=1 \
            M80_BIN="$M80_BIN" \
            bash scripts/bench-cold-launch.sh
        append_concurrent_rows "$egress" "$OUTDIR/concurrent.csv"
    done
done

write_summary_json
echo "density CSV: $DENSITY_CSV"
echo "density snapshot: $DENSITY_JSON"
