#!/usr/bin/env bash
# Cold-launch bench harness.
#
# Runs N launches per cell across two image kinds × two host load levels,
# captures end-to-end wallclock AND per-phase timings, and produces a
# JSON snapshot for diff/regression-gating via scripts/bench-summary.py.
#
# This is the foundation of the perf-bench push (m80-ekbk B0):
#   - extended percentiles (P50/P75/P90/P95/P99/P99.9/max), histograms
#   - bootstrap 95% CI on the median, 2σ outlier flagging
#   - SWEEP=<var>            one-variable sweeps emit sweep-<var>.csv
#   - CONCURRENT=N           N parallel admissions, wall-time-to-all-ready
#   - --cold-isolation       drop page caches between runs for cold-cold
#   - TASKSET / CPU_GOVERNOR best-effort isolation against host noise
#   - WARMUP=N               configurable warmup discard
#   - --dry-run              print the plan without running
#   - per-phase JSON event stream alongside CSV
#
# Requirements (only when actually running launches):
#   - KVM host, sudo NOPASSWD, /opt/firecracker/bin/{firecracker,jailer},
#     mkfs.ext4, unsquashfs, busybox-static (minimal kind), curl.
#   - stress-ng installed (apt install stress-ng) unless SKIP_LOADED=1.
#   - Both images pre-built into IMAGE_BUILD_DIR_UBUNTU/IMAGE_BUILD_DIR_MINIMAL.
#
# Usage:
#   ./scripts/bench-cold-launch.sh
#   N=30 ./scripts/bench-cold-launch.sh
#   N=1000 ./scripts/bench-cold-launch.sh                  # tail-latency mode
#   SKIP_LOADED=1 ./scripts/bench-cold-launch.sh           # idle-only
#   KIND=minimal ./scripts/bench-cold-launch.sh
#   SWEEP=vcpu SWEEP_VALUES=1,2,4 ./scripts/bench-cold-launch.sh
#   CONCURRENT=4 ./scripts/bench-cold-launch.sh
#   ./scripts/bench-cold-launch.sh --cold-isolation
#   TASKSET=0-3 CPU_GOVERNOR=performance ./scripts/bench-cold-launch.sh
#   WARMUP=5 ./scripts/bench-cold-launch.sh
#   ./scripts/bench-cold-launch.sh --dry-run
#
# Why warmup matters: the first 2-3 launches per cell are colder than
# the steady state because page caches, TLB entries, and the host
# scheduler haven't reached a stable working set yet. Increase WARMUP
# (or use --cold-isolation to drop caches between runs and treat every
# launch as cold-cold).

set -euo pipefail

cd "$(dirname "$0")/.."

# Default knobs (override via env var or flag).
N="${N:-30}"
WARMUP="${WARMUP:-2}"
KIND="${KIND:-both}"
SKIP_LOADED="${SKIP_LOADED:-0}"
STRESS_PROCS="${STRESS_PROCS:-$(nproc)}"
KERNEL_KIND="${KERNEL_KIND:-${M80_KERNEL_KIND:-stock}}"
IMAGE_UBUNTU="${IMAGE_BUILD_DIR_UBUNTU:-/tmp/m80-build/ubuntu}"
IMAGE_MINIMAL="${IMAGE_BUILD_DIR_MINIMAL:-/tmp/m80-build/minimal}"
RESULT_CSV="crates/m80-firecracker/benches/cold-launch.csv"
PHASE_CSV="crates/m80-firecracker/benches/cold-launch-phases.csv"
SNAPSHOTS_DIR="crates/m80-firecracker/benches/snapshots"

# B0 knobs.
SWEEP="${SWEEP:-}"
SWEEP_VALUES="${SWEEP_VALUES:-}"
CONCURRENT="${CONCURRENT:-0}"
TASKSET="${TASKSET:-}"
CPU_GOVERNOR="${CPU_GOVERNOR:-}"
M80_BIN="${M80_BIN:-./target/release/m80}"
COLD_ISOLATION=0
DRY_RUN=0
# When set, emit a JSONL phase event stream alongside the CSVs.
PHASE_JSONL="${PHASE_JSONL:-}"

usage() {
    sed -n '2,/^set -euo pipefail/p' "$0" | sed -E 's/^# ?//;/^set -euo pipefail/d'
    cat <<'EOF'

ENV vars:
    N             samples per cell (default 30; 1000 for tail latency)
    WARMUP        warmup discards per cell (default 2)
    KIND          ubuntu|minimal|both (default both)
    KERNEL_KIND   stock|stripped (default stock)
    SKIP_LOADED   set to 1 to skip the stress-ng cell
    SWEEP         vcpu|mem_mib|kernel_kind|image_kind
    SWEEP_VALUES  comma-separated values for the active sweep
    CONCURRENT    N parallel admissions per attempt; 0 = sequential (default)
    TASKSET       cpu-list passed to taskset -c (e.g. 0-3)
    CPU_GOVERNOR  passed to cpupower frequency-set -g (e.g. performance)
    DRY_RUN       set to 1 to print the plan and exit (same as --dry-run)
    PHASE_JSONL   when set, append phase events as JSONL to this path
    M80_BIN       binary path (default ./target/release/m80; override for tests)

FLAGS:
    --cold-isolation     echo 3 > /proc/sys/vm/drop_caches between runs
    --dry-run            print the plan and exit; no sudo, no cargo, no launches
    --help, -h           show this message and exit
EOF
}

# Parse flags. Anything past flags is ignored — knobs come from env.
while [[ $# -gt 0 ]]; do
    case "$1" in
        --cold-isolation) COLD_ISOLATION=1; shift ;;
        --dry-run)        DRY_RUN=1; shift ;;
        --help|-h)        usage; exit 0 ;;
        *)                echo "unknown flag: $1" >&2; usage >&2; exit 2 ;;
    esac
done

# Resolve KIND list once.
KINDS=()
case "$KIND" in
    ubuntu)  KINDS=("ubuntu") ;;
    minimal) KINDS=("minimal") ;;
    both)    KINDS=("ubuntu" "minimal") ;;
    *)       echo "KIND must be ubuntu|minimal|both" >&2; exit 1 ;;
esac

LOADS=("idle")
[[ "$SKIP_LOADED" != "1" ]] && LOADS+=("loaded")

# ── Plan summary (always printed; --dry-run exits here) ─────────────────
plan_summary() {
    echo "=== bench-cold-launch plan ==="
    echo "  N=$N  WARMUP=$WARMUP  KERNEL_KIND=$KERNEL_KIND"
    echo "  kinds=${KINDS[*]}  loads=${LOADS[*]}"
    [[ -n "$SWEEP" ]]      && echo "  SWEEP=$SWEEP  SWEEP_VALUES=${SWEEP_VALUES:-<defaults>}"
    [[ "$CONCURRENT" -gt 0 ]] && echo "  CONCURRENT=$CONCURRENT (N parallel admissions)"
    [[ -n "$TASKSET" ]]    && echo "  TASKSET=$TASKSET (taskset -c)"
    [[ -n "$CPU_GOVERNOR" ]] && echo "  CPU_GOVERNOR=$CPU_GOVERNOR (cpupower frequency-set -g)"
    [[ "$COLD_ISOLATION" -eq 1 ]] && echo "  --cold-isolation: echo 3 > /proc/sys/vm/drop_caches between runs"
    [[ -n "$PHASE_JSONL" ]] && echo "  PHASE_JSONL=$PHASE_JSONL"
    echo "  output: $RESULT_CSV, $PHASE_CSV, $SNAPSHOTS_DIR/latest.json"
}

plan_summary

if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "(dry-run) skipping launches; no sudo, no cargo build, no I/O against host."
    exit 0
fi

# ── From here on we actually run launches ───────────────────────────────

mkdir -p "$(dirname "$RESULT_CSV")"

# Best-effort CPU governor pin (root-only; warn but continue).
if [[ -n "$CPU_GOVERNOR" ]]; then
    if command -v cpupower >/dev/null 2>&1; then
        sudo cpupower frequency-set -g "$CPU_GOVERNOR" >/dev/null 2>&1 \
            || echo "warning: cpupower frequency-set -g $CPU_GOVERNOR failed" >&2
    else
        echo "warning: CPU_GOVERNOR set but cpupower not installed; skipping" >&2
    fi
fi

# Optional `taskset -c <cpus>` prefix. Built as a list so it expands
# correctly under `sudo ENV=VAL ${TASKSET_PREFIX[@]} cmd`; sudo can't
# see shell functions, so a function wrapper here would break under sudo.
if [[ -n "$TASKSET" ]]; then
    TASKSET_PREFIX=(taskset -c "$TASKSET")
else
    TASKSET_PREFIX=()
fi

# Drop page caches between runs (root only).
drop_caches() {
    [[ "$COLD_ISOLATION" -eq 1 ]] || return 0
    sync
    sudo sh -c 'echo 3 > /proc/sys/vm/drop_caches' 2>/dev/null \
        || echo "warning: failed to drop page caches (sudo required)" >&2
}

ensure_wallclock_csv() {
    if [[ ! -f "$RESULT_CSV" ]]; then
        echo "timestamp,kind,kernel_kind,load,attempt,launch_ms,exit" > "$RESULT_CSV"
        return
    fi
    if [[ "$(head -n 1 "$RESULT_CSV")" == "timestamp,kind,load,attempt,launch_ms,exit" ]]; then
        local tmp
        tmp="$(mktemp)"
        awk -F, 'BEGIN { OFS="," }
            NR == 1 { print "timestamp","kind","kernel_kind","load","attempt","launch_ms","exit"; next }
            NF == 6 { print $1,$2,"stock",$3,$4,$5,$6; next }
            { print }
        ' "$RESULT_CSV" > "$tmp"
        mv "$tmp" "$RESULT_CSV"
    fi
}

ensure_phase_csv() {
    if [[ ! -f "$PHASE_CSV" ]]; then
        echo "timestamp,kind,kernel_kind,load,attempt,phase,elapsed_us" > "$PHASE_CSV"
        return
    fi
    if [[ "$(head -n 1 "$PHASE_CSV")" == "timestamp,kind,load,attempt,phase,elapsed_us" ]]; then
        local tmp
        tmp="$(mktemp)"
        awk -F, 'BEGIN { OFS="," }
            NR == 1 { print "timestamp","kind","kernel_kind","load","attempt","phase","elapsed_us"; next }
            NF == 6 { print $1,$2,"stock",$3,$4,$5,$6; next }
            { print }
        ' "$PHASE_CSV" > "$tmp"
        mv "$tmp" "$PHASE_CSV"
    fi
}

ensure_wallclock_csv
ensure_phase_csv

# Per-run temp dir scopes a clean CSV pair to this invocation.
RUN_TEMP_DIR=$(mktemp -d)
trap 'rm -rf "$RUN_TEMP_DIR"' EXIT
RUN_RESULT_CSV="$RUN_TEMP_DIR/cold-launch.csv"
RUN_PHASE_CSV="$RUN_TEMP_DIR/cold-launch-phases.csv"
echo "timestamp,kind,kernel_kind,load,attempt,launch_ms,exit" > "$RUN_RESULT_CSV"
echo "timestamp,kind,kernel_kind,load,attempt,phase,elapsed_us" > "$RUN_PHASE_CSV"

# Sweep CSV (only populated when SWEEP=<var> is set).
SWEEP_CSV=""
if [[ -n "$SWEEP" ]]; then
    SWEEP_CSV="crates/m80-firecracker/benches/sweep-${SWEEP}.csv"
    if [[ ! -f "$SWEEP_CSV" ]]; then
        echo "timestamp,kind,kernel_kind,load,attempt,sweep_var,sweep_value,launch_ms,exit" > "$SWEEP_CSV"
    fi
fi

# Concurrent CSV (only when CONCURRENT > 0).
CONCURRENT_CSV=""
if [[ "$CONCURRENT" -gt 0 ]]; then
    CONCURRENT_CSV="crates/m80-firecracker/benches/concurrent.csv"
    if [[ ! -f "$CONCURRENT_CSV" ]]; then
        echo "timestamp,kind,kernel_kind,load,attempt,concurrency,vm_index,launch_ms,exit" > "$CONCURRENT_CSV"
    fi
fi

if [[ "$SKIP_LOADED" != "1" ]] && ! command -v stress-ng >/dev/null 2>&1; then
    echo "stress-ng required for loaded cells; install or run with SKIP_LOADED=1" >&2
    exit 1
fi

# Build the CLI once (skip if M80_BIN already points at a built binary —
# e.g., the test harness pointing at a mock).
if [[ "$M80_BIN" == "./target/release/m80" && ! -x "$M80_BIN" ]]; then
    cargo build --release -p m80-cli >/dev/null
fi

start_stress() {
    stress-ng --cpu "$STRESS_PROCS" --quiet >/dev/null 2>&1 &
    local pid=$!
    sleep 1
    if ! kill -0 "$pid" 2>/dev/null; then
        echo "stress-ng failed to start" >&2
        return 1
    fi
    echo "$pid"
}

stop_stress() {
    local pid="$1"
    [[ -z "$pid" ]] && return 0
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
}

# Emit a JSONL phase event if PHASE_JSONL is set.
emit_phase_event() {
    [[ -n "$PHASE_JSONL" ]] || return 0
    local kind="$1" load="$2" attempt="$3" phase="$4" elapsed_us="$5"
    local now
    now="$(date +%s%N)"
    printf '{"timestamp_ns":%s,"kind":"%s","kernel_kind":"%s","load":"%s","attempt":%s,"phase":"%s","elapsed_us":%s}\n' \
        "$now" "$kind" "$KERNEL_KIND" "$load" "$attempt" "$phase" "$elapsed_us" \
        >> "$PHASE_JSONL"
}

# Run one launch. Echoes "<launch_ms>,<exit_code>".
run_one() {
    local kind="$1" load="$2" attempt="$3" image_dir="$4"
    local stderr_file
    stderr_file="$(mktemp)"

    drop_caches

    local start_ns end_ns elapsed_ms exit_code
    start_ns=$(date +%s%N)
    # Best-effort cleanup of prior run-dir before each attempt.
    sudo IMAGE_BUILD_DIR="$image_dir" \
         M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker \
         M80_JAILER_BIN=/opt/firecracker/bin/jailer \
         M80_JAILER_HARDEN_BIN="${M80_JAILER_HARDEN_BIN:-$PWD/target/release/m80-jailer-harden}" \
         M80_KERNEL_IMAGE="$image_dir/vmlinux" \
         M80_KERNEL_KIND="$KERNEL_KIND" \
         M80_ROOTFS_IMAGE="$image_dir/output.ext4" \
         M80_RUN_ROOT=/var/lib/m80-run \
         M80_FIRECRACKER_VERSION=v1.15.1 \
         M80_JAIL_UID="$(id -u)" \
         M80_JAIL_GID="$(getent group kvm | cut -d: -f3 || id -g)" \
         "${TASKSET_PREFIX[@]}" "$M80_BIN" cleanup >/dev/null 2>&1 || true

    if timeout 90 sudo M80_PHASE_TRACE=1 \
            IMAGE_BUILD_DIR="$image_dir" \
            M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker \
            M80_JAILER_BIN=/opt/firecracker/bin/jailer \
            M80_JAILER_HARDEN_BIN="${M80_JAILER_HARDEN_BIN:-$PWD/target/release/m80-jailer-harden}" \
            M80_KERNEL_IMAGE="$image_dir/vmlinux" \
            M80_KERNEL_KIND="$KERNEL_KIND" \
            M80_ROOTFS_IMAGE="$image_dir/output.ext4" \
            M80_RUN_ROOT=/var/lib/m80-run \
            M80_FIRECRACKER_VERSION=v1.15.1 \
            M80_JAIL_UID="$(id -u)" \
            M80_JAIL_GID="$(getent group kvm | cut -d: -f3 || id -g)" \
            "${TASKSET_PREFIX[@]}" "$M80_BIN" run \
            --egress none -- /bin/echo "bench-$attempt" \
            >/dev/null 2>"$stderr_file"; then
        exit_code=0
    else
        exit_code=$?
    fi
    end_ns=$(date +%s%N)
    elapsed_ms=$(( (end_ns - start_ns) / 1000000 ))

    local ts
    ts="$(date -Iseconds)"
    while IFS= read -r line; do
        case "$line" in
            "M80_PHASE "*) ;;
            *) continue ;;
        esac
        local name us
        name="${line#*name=}"; name="${name%% *}"
        us="${line##*elapsed_us=}"; us="${us%%[!0-9]*}"
        [[ "$name" != "$line" && "$us" =~ ^[0-9]+$ ]] || continue
        echo "$ts,$kind,$KERNEL_KIND,$load,$attempt,$name,$us" >> "$PHASE_CSV"
        echo "$ts,$kind,$KERNEL_KIND,$load,$attempt,$name,$us" >> "$RUN_PHASE_CSV"
        emit_phase_event "$kind" "$load" "$attempt" "$name" "$us"
    done < "$stderr_file"
    while IFS= read -r line; do
        case "$line" in
            "M80_GUEST_BOOT "*) ;;
            *) continue ;;
        esac
        local name elapsed_us delta_us
        name="${line#*name=}"; name="${name%% *}"
        elapsed_us="${line#*elapsed_us=}"; elapsed_us="${elapsed_us%% *}"
        delta_us="${line#*delta_us=}"; delta_us="${delta_us%%[!0-9]*}"
        [[ "$name" != "$line" && "$elapsed_us" =~ ^[0-9]+$ && "$delta_us" =~ ^[0-9]+$ ]] || continue
        echo "$ts,$kind,$KERNEL_KIND,$load,$attempt,guest_elapsed_$name,$elapsed_us" >> "$PHASE_CSV"
        echo "$ts,$kind,$KERNEL_KIND,$load,$attempt,guest_elapsed_$name,$elapsed_us" >> "$RUN_PHASE_CSV"
        echo "$ts,$kind,$KERNEL_KIND,$load,$attempt,guest_delta_$name,$delta_us" >> "$PHASE_CSV"
        echo "$ts,$kind,$KERNEL_KIND,$load,$attempt,guest_delta_$name,$delta_us" >> "$RUN_PHASE_CSV"
        emit_phase_event "$kind" "$load" "$attempt" "guest_elapsed_$name" "$elapsed_us"
        emit_phase_event "$kind" "$load" "$attempt" "guest_delta_$name" "$delta_us"
    done < "$stderr_file"

    rm -f "$stderr_file"
    echo "$elapsed_ms,$exit_code"
}

# Run CONCURRENT VMs in parallel; emit wall-time-to-all-ready per attempt.
run_concurrent_cell() {
    local kind="$1" load="$2" image_dir="$3"
    local total=$((N + WARMUP))
    for attempt in $(seq 1 "$total"); do
        local pids=() outs=()
        local launch_start launch_end
        launch_start=$(date +%s%N)
        for vm in $(seq 0 $((CONCURRENT - 1))); do
            local out
            out="$(mktemp)"
            outs+=("$out")
            (run_one "$kind" "$load" "${attempt}_${vm}" "$image_dir" > "$out") &
            pids+=($!)
        done
        for pid in "${pids[@]}"; do
            wait "$pid" || true
        done
        launch_end=$(date +%s%N)
        local all_ready_ms=$(( (launch_end - launch_start) / 1000000 ))

        if (( attempt > WARMUP )); then
            for vm in $(seq 0 $((CONCURRENT - 1))); do
                local row launch_ms exit_code
                row="$(cat "${outs[$vm]}")"
                launch_ms="${row%,*}"
                exit_code="${row#*,}"
                echo "$(date -Iseconds),$kind,$KERNEL_KIND,$load,$attempt,$CONCURRENT,$vm,$launch_ms,$exit_code" \
                    >> "$CONCURRENT_CSV"
            done
            printf '  [concurrent=%d] attempt=%d wall_time_to_all_ready=%dms\n' \
                "$CONCURRENT" "$attempt" "$all_ready_ms"
        fi

        for out in "${outs[@]}"; do
            rm -f "$out"
        done
    done
}

run_cell() {
    local kind="$1" load="$2" image_dir="$3"
    local stress_pid=""

    if [[ "$load" == "loaded" ]]; then
        echo "  starting stress-ng --cpu $STRESS_PROCS"
        stress_pid="$(start_stress)" || return 1
    fi

    if [[ "$CONCURRENT" -gt 0 ]]; then
        run_concurrent_cell "$kind" "$load" "$image_dir"
        stop_stress "$stress_pid"
        return 0
    fi

    local ok_results=()
    local fail_count=0
    local total=$((N + WARMUP))
    for i in $(seq 1 "$total"); do
        local row launch_ms exit_code
        row="$(run_one "$kind" "$load" "$i" "$image_dir")"
        launch_ms="${row%,*}"
        exit_code="${row#*,}"

        if (( i > WARMUP )); then
            echo "$(date -Iseconds),$kind,$KERNEL_KIND,$load,$i,$launch_ms,$exit_code" >> "$RESULT_CSV"
            echo "$(date -Iseconds),$kind,$KERNEL_KIND,$load,$i,$launch_ms,$exit_code" >> "$RUN_RESULT_CSV"
            if [[ -n "$SWEEP" ]]; then
                echo "$(date -Iseconds),$kind,$KERNEL_KIND,$load,$i,$SWEEP,$CUR_SWEEP_VAL,$launch_ms,$exit_code" \
                    >> "$SWEEP_CSV"
            fi
            if [[ "$exit_code" == "0" ]]; then
                ok_results+=("$launch_ms")
            else
                fail_count=$((fail_count + 1))
            fi
        fi
    done

    stop_stress "$stress_pid"
    summarize "$kind" "$load" "$fail_count" "${ok_results[@]}"
}

summarize() {
    local kind="$1" load="$2" fail_count="$3"
    shift 3
    if [[ $# -eq 0 ]]; then
        printf '  %-8s %-6s  no successful launches (failures=%s)\n' "$kind" "$load" "$fail_count"
        return
    fi
    local sorted count
    sorted=$(printf '%s\n' "$@" | sort -n)
    count=$(printf '%s\n' "$sorted" | wc -l)
    local p50_idx p95_idx p99_idx
    p50_idx=$(( count / 2 ))
    p95_idx=$(( count * 95 / 100 ))
    p99_idx=$(( count * 99 / 100 ))
    (( p50_idx == 0 )) && p50_idx=1
    (( p95_idx == 0 )) && p95_idx=1
    (( p99_idx == 0 )) && p99_idx=1
    local p50 p95 p99 max
    p50=$(printf '%s\n' "$sorted" | sed -n "${p50_idx}p")
    p95=$(printf '%s\n' "$sorted" | sed -n "${p95_idx}p")
    p99=$(printf '%s\n' "$sorted" | sed -n "${p99_idx}p")
    max=$(printf '%s\n' "$sorted" | tail -1)
    local fail_note=""
    (( fail_count > 0 )) && fail_note=" *failures=$fail_count*"
    printf '  %-8s %-6s  P50=%5sms  P95=%5sms  P99=%5sms  MAX=%5sms  (n=%s)%s\n' \
        "$kind" "$load" "$p50" "$p95" "$p99" "$max" "$count" "$fail_note"
}

phase_summary() {
    echo
    echo "=== per-phase P50 (us) ==="
    python3 scripts/bench-summary.py summarize \
        --csv-wallclock "$RUN_RESULT_CSV" \
        --csv-phase "$RUN_PHASE_CSV"
}

# ── Main loop ──────────────────────────────────────────────────────────

# Resolve sweep values. The default sweep for vcpu is 1/2/4; for mem_mib
# 256/512/1024; for kernel_kind stock/stripped.
resolve_sweep_values() {
    if [[ -n "$SWEEP_VALUES" ]]; then
        IFS=',' read -r -a vals <<<"$SWEEP_VALUES"
        printf '%s\n' "${vals[@]}"
        return
    fi
    case "$SWEEP" in
        vcpu)         printf '%s\n' 1 2 4 ;;
        mem_mib)      printf '%s\n' 256 512 1024 ;;
        kernel_kind)  printf '%s\n' stock stripped ;;
        image_kind)   printf '%s\n' minimal ubuntu ;;
        *)            echo "unknown SWEEP=$SWEEP (vcpu|mem_mib|kernel_kind|image_kind)" >&2; exit 1 ;;
    esac
}

if [[ -n "$SWEEP" ]]; then
    echo "=== sweep: $SWEEP ==="
    while IFS= read -r CUR_SWEEP_VAL; do
        echo "--- $SWEEP=$CUR_SWEEP_VAL ---"
        # Sweep variables that affect kernel kind or image kind handled below.
        case "$SWEEP" in
            kernel_kind) KERNEL_KIND="$CUR_SWEEP_VAL" ;;
            image_kind)  KINDS=("$CUR_SWEEP_VAL") ;;
            # vcpu / mem_mib are not yet plumbed through m80 run flags;
            # the sweep CSV records what was requested for observability.
        esac
        for k in "${KINDS[@]}"; do
            case "$k" in
                ubuntu)  dir="$IMAGE_UBUNTU" ;;
                minimal) dir="$IMAGE_MINIMAL" ;;
            esac
            if [[ ! -f "$dir/vmlinux" || ! -f "$dir/output.ext4.manifest.json" ]]; then
                echo "skip $k: missing $dir/vmlinux or manifest. Build first."
                continue
            fi
            for l in "${LOADS[@]}"; do
                run_cell "$k" "$l" "$dir"
            done
        done
    done < <(resolve_sweep_values)
else
    echo "=== bench-cold-launch (N=$N per cell, warmup=$WARMUP) ==="
    for k in "${KINDS[@]}"; do
        case "$k" in
            ubuntu)  dir="$IMAGE_UBUNTU" ;;
            minimal) dir="$IMAGE_MINIMAL" ;;
        esac
        if [[ ! -f "$dir/vmlinux" || ! -f "$dir/output.ext4.manifest.json" ]]; then
            echo "skip $k: missing $dir/vmlinux or manifest. Build first."
            continue
        fi
        for l in "${LOADS[@]}"; do
            echo "--- cell: $k / $l ---"
            run_cell "$k" "$l" "$dir"
        done
    done
fi

phase_summary

mkdir -p "$SNAPSHOTS_DIR"
SNAPSHOT_OUT="$SNAPSHOTS_DIR/$(date -Iseconds).json"
python3 scripts/bench-summary.py compute \
    --csv-wallclock "$RUN_RESULT_CSV" \
    --csv-phase "$RUN_PHASE_CSV" \
    --output "$SNAPSHOT_OUT"
ln -sf "$(basename "$SNAPSHOT_OUT")" "$SNAPSHOTS_DIR/latest.json"

echo
echo "wallclock CSV:  $RESULT_CSV"
echo "per-phase CSV:  $PHASE_CSV"
[[ -n "$SWEEP_CSV" ]]      && echo "sweep CSV:      $SWEEP_CSV"
[[ -n "$CONCURRENT_CSV" ]] && echo "concurrent CSV: $CONCURRENT_CSV"
[[ -n "$PHASE_JSONL" ]]    && echo "phase JSONL:    $PHASE_JSONL"
echo "snapshot saved: $SNAPSHOT_OUT"
echo
echo "# next-run diff: python3 scripts/bench-summary.py diff $SNAPSHOT_OUT <new-run.json>"
