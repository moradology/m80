#!/usr/bin/env bash
# Cold-launch bench harness for m80-6a0q.6.
#
# Runs N launches per cell across two image kinds × two host load levels,
# captures end-to-end wallclock AND per-phase timings (via
# M80_PHASE_TRACE=1 + parsing M80_PHASE lines on stderr), prints
# P50/P95/max per cell, appends rows to two CSVs:
#
#   - crates/m80-firecracker/benches/cold-launch.csv
#       per-launch wallclock: timestamp,kind,load,attempt,launch_ms,exit
#   - crates/m80-firecracker/benches/cold-launch-phases.csv
#       long format: timestamp,kind,load,attempt,phase,elapsed_us
#
# Requirements (same as scripts/smoke.sh):
#   - KVM host, sudo NOPASSWD, /opt/firecracker/bin/{firecracker,jailer},
#     mkfs.ext4, unsquashfs, busybox-static (for minimal kind), curl.
#   - stress-ng installed (apt install stress-ng) — unless SKIP_LOADED=1.
#   - Both images pre-built into separate IMAGE_BUILD_DIR_UBUNTU and
#     IMAGE_BUILD_DIR_MINIMAL directories (run scripts/smoke.sh per kind
#     to seed; default dirs match smoke.sh's M80_IMAGE_KIND convention).
#
# Usage:
#   ./scripts/bench-cold-launch.sh
#   N=30 ./scripts/bench-cold-launch.sh                    # custom run count
#   SKIP_LOADED=1 ./scripts/bench-cold-launch.sh           # idle-only
#   KIND=minimal ./scripts/bench-cold-launch.sh            # one kind only
#
# Knobs:
#   N                                Default 30. Number of launches per cell.
#   IMAGE_BUILD_DIR_UBUNTU           Default /tmp/m80-build/ubuntu
#   IMAGE_BUILD_DIR_MINIMAL          Default /tmp/m80-build/minimal
#   KIND                             ubuntu|minimal|both (default both)
#   SKIP_LOADED                      Skip the stress-ng cell when set
#   STRESS_PROCS                     Default $(nproc). stress-ng --cpu N
#
# Caveats:
#   - "launch_ms" is wallclock from `m80 launch` invocation to exit, not a
#     precise InstanceStart-to-ready measurement. The per-phase CSV
#     attributes the within-binary slice; spawn + teardown are external.
#   - The first 2 launches per cell are discarded as warmup.

set -euo pipefail

cd "$(dirname "$0")/.."

N="${N:-30}"
WARMUP=2
KIND="${KIND:-both}"
SKIP_LOADED="${SKIP_LOADED:-0}"
STRESS_PROCS="${STRESS_PROCS:-$(nproc)}"
IMAGE_UBUNTU="${IMAGE_BUILD_DIR_UBUNTU:-/tmp/m80-build/ubuntu}"
IMAGE_MINIMAL="${IMAGE_BUILD_DIR_MINIMAL:-/tmp/m80-build/minimal}"
RESULT_CSV="crates/m80-firecracker/benches/cold-launch.csv"
PHASE_CSV="crates/m80-firecracker/benches/cold-launch-phases.csv"

mkdir -p "$(dirname "$RESULT_CSV")"
[[ -f "$RESULT_CSV" ]] || echo "timestamp,kind,load,attempt,launch_ms,exit" > "$RESULT_CSV"
[[ -f "$PHASE_CSV" ]] || echo "timestamp,kind,load,attempt,phase,elapsed_us" > "$PHASE_CSV"

# Hard-fail early on missing dependencies. The previous "background
# stress-ng with stderr suppressed" pattern silently turned loaded cells
# into idle cells when stress-ng was absent.
if [[ "$SKIP_LOADED" != "1" ]]; then
    if ! command -v stress-ng >/dev/null 2>&1; then
        echo "stress-ng required for loaded cells; install (apt install stress-ng) or run with SKIP_LOADED=1" >&2
        exit 1
    fi
fi

# Build the CLI once.
cargo build --release -p m80-cli >/dev/null

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

# Run one launch, return timing + write phase events to PHASE_CSV.
# Echoes "<launch_ms>,<exit_code>".
run_one() {
    local kind="$1" load="$2" attempt="$3" image_dir="$4"
    local stderr_file
    stderr_file="$(mktemp)"

    sudo IMAGE_BUILD_DIR="$image_dir" \
         M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker \
         M80_JAILER_BIN=/opt/firecracker/bin/jailer \
         M80_KERNEL_IMAGE="$image_dir/vmlinux" \
         M80_ROOTFS_IMAGE="$image_dir/output.ext4" \
         M80_RUN_ROOT=/var/lib/m80-run \
         M80_FIRECRACKER_VERSION=v1.15.1 \
         M80_JAIL_UID="$(id -u)" \
         M80_JAIL_GID="$(getent group kvm | cut -d: -f3 || id -g)" \
         ./target/release/m80 cleanup >/dev/null 2>&1 || true

    local start_ns end_ns elapsed_ms exit_code
    start_ns=$(date +%s%N)
    if timeout 90 sudo M80_PHASE_TRACE=1 \
            IMAGE_BUILD_DIR="$image_dir" \
            M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker \
            M80_JAILER_BIN=/opt/firecracker/bin/jailer \
            M80_KERNEL_IMAGE="$image_dir/vmlinux" \
            M80_ROOTFS_IMAGE="$image_dir/output.ext4" \
            M80_RUN_ROOT=/var/lib/m80-run \
            M80_FIRECRACKER_VERSION=v1.15.1 \
            M80_JAIL_UID="$(id -u)" \
            M80_JAIL_GID="$(getent group kvm | cut -d: -f3 || id -g)" \
            ./target/release/m80 launch \
            --network noegress -- /bin/echo "bench-$attempt" \
            >/dev/null 2>"$stderr_file"; then
        exit_code=0
    else
        exit_code=$?
    fi
    end_ns=$(date +%s%N)
    elapsed_ms=$(( (end_ns - start_ns) / 1000000 ))

    # Parse M80_PHASE lines on stderr -> phase CSV.
    local ts
    ts="$(date -Iseconds)"
    while IFS= read -r line; do
        local name us
        name="${line#*name=}"; name="${name%% *}"
        us="${line##*elapsed_us=}"; us="${us%%[!0-9]*}"
        echo "$ts,$kind,$load,$attempt,$name,$us" >> "$PHASE_CSV"
    done < <(grep '^M80_PHASE ' "$stderr_file" || true)

    rm -f "$stderr_file"
    echo "$elapsed_ms,$exit_code"
}

run_cell() {
    local kind="$1" load="$2" image_dir="$3"
    local stress_pid=""

    if [[ "$load" == "loaded" ]]; then
        echo "  starting stress-ng --cpu $STRESS_PROCS"
        stress_pid="$(start_stress)" || return 1
    fi

    # Track successes (for percentile computation) AND failures separately.
    local ok_results=()
    local fail_count=0
    local total=$((N + WARMUP))
    for i in $(seq 1 "$total"); do
        local row launch_ms exit_code
        row="$(run_one "$kind" "$load" "$i" "$image_dir")"
        launch_ms="${row%,*}"
        exit_code="${row#*,}"

        if (( i > WARMUP )); then
            echo "$(date -Iseconds),$kind,$load,$i,$launch_ms,$exit_code" >> "$RESULT_CSV"
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
    local sorted
    sorted=$(printf '%s\n' "$@" | sort -n)
    local count
    count=$(printf '%s\n' "$sorted" | wc -l)
    local p50_idx p95_idx
    p50_idx=$(( count / 2 ))
    p95_idx=$(( count * 95 / 100 ))
    (( p50_idx == 0 )) && p50_idx=1
    (( p95_idx == 0 )) && p95_idx=1
    local p50 p95 max
    p50=$(printf '%s\n' "$sorted" | sed -n "${p50_idx}p")
    p95=$(printf '%s\n' "$sorted" | sed -n "${p95_idx}p")
    max=$(printf '%s\n' "$sorted" | tail -1)
    local fail_note=""
    if (( fail_count > 0 )); then
        fail_note=" *failures=$fail_count*"
    fi
    printf '  %-8s %-6s  P50=%5sms  P95=%5sms  MAX=%5sms  (n=%s)%s\n' \
        "$kind" "$load" "$p50" "$p95" "$max" "$count" "$fail_note"
}

# Per-phase summary + "useful_ms" rollup (total wallclock minus stop_bounded
# since SendCtrlAltDel timeout dominates total today; replacing with a
# vsock graceful-stop is a tracked v0.2 epic).
phase_summary() {
    echo
    echo "=== per-phase P50 (us) ==="
    python3 - <<'PY'
import csv, statistics, collections, sys
rows = collections.defaultdict(list)
try:
    with open("crates/m80-firecracker/benches/cold-launch-phases.csv") as f:
        r = csv.DictReader(f)
        for row in r:
            key = (row["kind"], row["load"], row["phase"])
            try:
                rows[key].append(int(row["elapsed_us"]))
            except ValueError:
                pass
except FileNotFoundError:
    sys.exit(0)
phases_seen = sorted({k[2] for k in rows})
header = ("ubuntu/idle","ubuntu/loaded","minimal/idle","minimal/loaded")
print(f'  {"phase":<26} ' + "  ".join(f"{k:>14}" for k in header))
for ph in phases_seen:
    cells = []
    for kind, load in [("ubuntu","idle"),("ubuntu","loaded"),("minimal","idle"),("minimal","loaded")]:
        vals = rows.get((kind,load,ph), [])
        cells.append(f"{int(statistics.median(vals)):>11} us" if vals else f"{'':>14}")
    print(f"  {ph:<26} " + "  ".join(cells))

# Rollup: useful_ms = sum of all phases EXCEPT stop_bounded. This is the
# meaningful "launch+exec time" — what we'd see if SendCtrlAltDel weren't
# wedging the host for 30 s on every run.
print()
print("=== useful_ms (total - stop_bounded, P50) ===")
EXCLUDE = {"stop_bounded"}
print(f'  {"":<26} ' + "  ".join(f"{k:>14}" for k in header))
useful_per_attempt = collections.defaultdict(lambda: collections.defaultdict(int))
for (kind, load, phase), vals in rows.items():
    if phase in EXCLUDE:
        continue
    # Sum per-attempt: re-load CSV walking attempt.
useful_per_attempt = collections.defaultdict(lambda: collections.defaultdict(int))
with open("crates/m80-firecracker/benches/cold-launch-phases.csv") as f:
    for row in csv.DictReader(f):
        key = (row["kind"], row["load"])
        if row["phase"] in EXCLUDE:
            continue
        try:
            useful_per_attempt[key][row["attempt"]] += int(row["elapsed_us"])
        except ValueError:
            pass
cells = []
for kind, load in [("ubuntu","idle"),("ubuntu","loaded"),("minimal","idle"),("minimal","loaded")]:
    vals = list(useful_per_attempt.get((kind, load), {}).values())
    if vals:
        cells.append(f"{int(statistics.median(vals)/1000):>11} ms")
    else:
        cells.append(f"{'':>14}")
print(f'  {"useful_ms (P50)":<26} ' + "  ".join(cells))
PY
}

echo "=== bench-cold-launch (N=$N per cell, warmup=$WARMUP) ==="
echo

KINDS=()
case "$KIND" in
    ubuntu)  KINDS=("ubuntu") ;;
    minimal) KINDS=("minimal") ;;
    both)    KINDS=("ubuntu" "minimal") ;;
    *)       echo "KIND must be ubuntu|minimal|both"; exit 1 ;;
esac

LOADS=("idle")
[[ "$SKIP_LOADED" != "1" ]] && LOADS+=("loaded")

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

phase_summary

echo
echo "wallclock CSV:  $RESULT_CSV"
echo "per-phase CSV:  $PHASE_CSV"
