#!/usr/bin/env bash
# bench-extras.sh — orchestrator for the B1-B10 perf sub-epics.
#
# Each --mode produces a CSV consumable by scripts/bench-summary.py
# (compute_throughput / compute_memory / compute_teardown). Modes:
#
#   --throughput    B2: sustain N ops/window; emit throughput.csv
#   --memory        B4: sample firecracker RSS during a run; emit memory.csv
#   --teardown      B8: time stop_bounded / residue_cleanup / release; emit
#                       teardown.csv
#   --boot-decomp   B1: kernel-boot initcall decomposition (guest-side)
#   --long-tail     B10: N=1000+ histogram report from existing snapshot
#   --density       B3: concurrent-ladder sweep (1, 2, 4, 8, 16, 32)
#
# Each mode runs to completion, writes its CSV, and prints a one-line
# summary. The real-KVM data gathering happens on the privileged runner
# (m80-16hx7); the mock path verifies the orchestration end-to-end.

set -euo pipefail

cd "$(dirname "$0")/.."

M80_BIN="${M80_BIN:-./target/release/m80}"
KIND="${KIND:-minimal}"
N="${N:-50}"
WARMUP="${WARMUP:-2}"
WINDOW_SEC="${WINDOW_SEC:-10}"
LADDER="${LADDER:-1,2,4,8,16}"
OUTDIR="crates/m80-firecracker/benches"

mkdir -p "$OUTDIR"

usage() {
    cat <<'EOF'
bench-extras.sh — orchestrator for the m80 perf sub-epics.

Usage: scripts/bench-extras.sh --MODE [env-vars]

Modes:
  --throughput     run N ops over WINDOW_SEC seconds; emit throughput.csv
  --memory         sample firecracker RSS during a run; emit memory.csv
  --teardown       time the stop_bounded / residue_cleanup / release phases
  --boot-decomp    request guest-side initcall decomposition from m80-guestd
  --long-tail      summarize tail latency from latest snapshot
  --density        concurrent-ladder sweep (1, 2, 4, 8, 16 by default)
  --help           show this message

Env vars (with their defaults):
  M80_BIN=./target/release/m80
  KIND=minimal       ubuntu | minimal
  N=50               total attempts per mode (where applicable)
  WARMUP=2           warmup discards
  WINDOW_SEC=10      throughput observation window
  LADDER=1,2,4,8,16  concurrency steps for --density

Example:
  N=20 ./scripts/bench-extras.sh --throughput
EOF
}

# Invoke the m80 binary the same way bench-cold-launch.sh does.
m80_run() {
    sudo M80_PHASE_TRACE=1 \
        M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker \
        M80_JAILER_BIN=/opt/firecracker/bin/jailer \
        M80_JAILER_HARDEN_BIN="${M80_JAILER_HARDEN_BIN:-$PWD/target/release/m80-jailer-harden}" \
        M80_RUN_ROOT=/var/lib/m80-run \
        M80_FIRECRACKER_VERSION=v1.15.1 \
        M80_JAIL_UID="$(id -u)" \
        M80_JAIL_GID="$(getent group kvm | cut -d: -f3 || id -g)" \
        "$M80_BIN" run --egress none -- "$@"
}

mode_throughput() {
    local out="$OUTDIR/throughput.csv"
    echo "timestamp_unix_ms,kind,op,latency_ms,exit" > "$out"
    local deadline_ms=$(( $(date +%s%N) / 1000000 + WINDOW_SEC * 1000 ))
    local ops=0
    while (( $(date +%s%N) / 1000000 < deadline_ms )); do
        local start_ns end_ns lat_ms exit_code
        start_ns=$(date +%s%N)
        if m80_run /bin/echo throughput-$ops >/dev/null 2>&1; then
            exit_code=0
        else
            exit_code=$?
        fi
        end_ns=$(date +%s%N)
        lat_ms=$(( (end_ns - start_ns) / 1000000 ))
        echo "$(( end_ns / 1000000 )),$KIND,exec,$lat_ms,$exit_code" >> "$out"
        ops=$((ops + 1))
        # Stop at N attempts even if WINDOW_SEC budget remains, so tests
        # finish promptly.
        (( ops >= N )) && break
    done
    echo "throughput: ran $ops ops in ≤${WINDOW_SEC}s; CSV: $out"
    python3 -c "
import sys; sys.path.insert(0, 'scripts')
from importlib.machinery import SourceFileLoader
m = SourceFileLoader('bs', 'scripts/bench-summary.py').load_module()
with open('$out') as f:
    r = m.compute_throughput(f)
print('  result:', r)
"
}

mode_memory() {
    local out="$OUTDIR/memory.csv"
    echo "timestamp,vm_id,rss_kb,vm_count" > "$out"
    # Sample firecracker children under /var/lib/m80-run. Run with a
    # launch in flight in another shell for non-zero samples.
    local samples="${MEMORY_SAMPLES:-5}"
    for i in $(seq 1 "$samples"); do
        local pids
        pids="$(pgrep -f 'firecracker.*--api-sock' || true)"
        local vm_count
        vm_count=$(echo "$pids" | grep -c . || echo 0)
        for pid in $pids; do
            local rss
            rss=$(awk '/^VmRSS:/{print $2}' "/proc/$pid/status" 2>/dev/null || echo 0)
            echo "$(date +%s),vm-$pid,$rss,$vm_count" >> "$out"
        done
        sleep 1
    done
    echo "memory: $samples samples written to $out"
    python3 -c "
import sys; sys.path.insert(0, 'scripts')
from importlib.machinery import SourceFileLoader
m = SourceFileLoader('bs', 'scripts/bench-summary.py').load_module()
with open('$out') as f:
    r = m.compute_memory(f)
print('  result:', r)
"
}

mode_teardown() {
    local out="$OUTDIR/teardown.csv"
    echo "timestamp,kind,attempt,phase,elapsed_us" > "$out"
    local n="$N"
    for i in $(seq 1 "$n"); do
        local stderr_file
        stderr_file="$(mktemp)"
        m80_run /bin/echo teardown-$i >/dev/null 2>"$stderr_file" || true
        # Pull the relevant teardown phases from the M80_PHASE stream.
        grep -E 'stop_bounded|residue_cleanup|force_kill|release|stop_release' \
            "$stderr_file" 2>/dev/null | while IFS= read -r line; do
            local name us
            name="${line#*name=}"; name="${name%% *}"
            us="${line##*elapsed_us=}"; us="${us%%[!0-9]*}"
            echo "$(date +%s),$KIND,$i,$name,$us" >> "$out"
        done
        rm -f "$stderr_file"
    done
    echo "teardown: $n attempts written to $out"
    python3 -c "
import sys; sys.path.insert(0, 'scripts')
from importlib.machinery import SourceFileLoader
m = SourceFileLoader('bs', 'scripts/bench-summary.py').load_module()
with open('$out') as f:
    r = m.compute_teardown(f)
print('  result:', r)
"
}

mode_boot_decomp() {
    # B1: ask m80 to run a guest-side `dmesg` inside the VM and capture
    # the kernel's initcall timing. The output goes to boot-decomp.txt
    # for human review; structured parsing is left for a follow-up.
    local out="$OUTDIR/boot-decomp.txt"
    echo "# m80 boot decomposition $(date -Iseconds)" > "$out"
    m80_run /bin/dmesg >> "$out" 2>&1 || true
    echo "boot-decomp: written to $out (parse with 'grep initcall' for per-call us)"
}

mode_long_tail() {
    # B10: report the tail-latency profile from the latest snapshot.
    local snap="$OUTDIR/snapshots/latest.json"
    if [[ ! -f "$snap" ]]; then
        echo "no snapshot found at $snap; run bench-cold-launch.sh first" >&2
        return 1
    fi
    python3 -c "
import json, sys
with open('$snap') as f:
    data = json.load(f)['data']
print('=== long-tail summary (from latest snapshot) ===')
for kind, loads in data['wallclock'].items():
    for load, w in loads.items():
        print(f'  {kind}/{load}  N={w[\"count\"]}  P50={w[\"p50\"]}ms  P95={w[\"p95\"]}ms  P99={w[\"p99\"]}ms  P99.9={w[\"p999\"]}ms  MAX={w[\"max\"]}ms  outliers={w[\"outliers\"]}')
print()
print('=== per-phase tail (P99/P99.9 in us) ===')
for kind, loads in data['phases'].items():
    for load, phases in loads.items():
        for ph, s in sorted(phases.items()):
            print(f'  {kind}/{load}/{ph:<28}  P99={s[\"p99_us\"]:>8}us  P99.9={s[\"p999_us\"]:>8}us  MAX={s[\"max_us\"]:>8}us  buckets={len(s.get(\"histogram_us\", []))}')
"
}

mode_density() {
    # B3: concurrent ladder. Just calls bench-cold-launch.sh with
    # CONCURRENT=<c> for each step in $LADDER and aggregates from
    # concurrent.csv.
    rm -f "$OUTDIR/concurrent.csv"
    IFS=',' read -r -a steps <<<"$LADDER"
    for c in "${steps[@]}"; do
        echo "--- density step: CONCURRENT=$c ---"
        M80_BIN="$M80_BIN" \
            CONCURRENT="$c" N="$N" WARMUP="$WARMUP" \
            KIND="$KIND" SKIP_LOADED=1 \
            bash scripts/bench-cold-launch.sh \
            | grep -E 'wall_time_to_all_ready' || true
    done
    if [[ -f "$OUTDIR/concurrent.csv" ]]; then
        python3 -c "
import sys; sys.path.insert(0, 'scripts')
from importlib.machinery import SourceFileLoader
m = SourceFileLoader('bs', 'scripts/bench-summary.py').load_module()
with open('$OUTDIR/concurrent.csv') as f:
    r = m.compute_concurrent(f)
print('=== density ladder summary ===')
for conc, s in sorted(r.items(), key=lambda kv: int(kv[0])):
    print(f'  concurrency={conc}  wall_time_to_all_ready_p50={s[\"wall_time_to_all_ready_p50_ms\"]}ms  per_vm_p95={s[\"per_vm_p95_ms\"]}ms  vms={s[\"vm_count\"]}')
"
    fi
}

case "${1:-}" in
    --throughput) mode_throughput ;;
    --memory)     mode_memory ;;
    --teardown)   mode_teardown ;;
    --boot-decomp) mode_boot_decomp ;;
    --long-tail)  mode_long_tail ;;
    --density)    mode_density ;;
    --help|-h|"") usage ;;
    *)            echo "unknown mode: $1" >&2; usage >&2; exit 2 ;;
esac
