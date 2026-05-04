#!/usr/bin/env bash
# Cold-launch bench harness for m80-6a0q.6.
#
# Runs N launches per cell across two image kinds × two host load levels,
# records phase_12b ready-probe wallclock timings, prints P50/P95/max
# per cell, appends a CSV row per launch to crates/m80-firecracker/benches/cold-launch.csv.
#
# Requirements (same as scripts/smoke.sh):
#   - KVM host, sudo NOPASSWD, /opt/firecracker/bin/{firecracker,jailer},
#     mkfs.ext4, unsquashfs, busybox-static (for minimal kind), curl.
#   - Both images pre-built into separate IMAGE_BUILD_DIR_UBUNTU and
#     IMAGE_BUILD_DIR_MINIMAL directories (see "build images" section below
#     or run scripts/smoke.sh first to seed the ubuntu cache).
#   - For minimal kind: m80-guestd built with --target x86_64-unknown-linux-musl.
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
# Output:
#   stdout:                          per-cell summary table
#   crates/m80-firecracker/benches/cold-launch.csv
#                                    one row per launch
#                                    cols: timestamp,kind,load,attempt,launch_ms,exit
#
# Caveats:
#   - "launch_ms" is the wallclock from `m80 launch` invocation to exit,
#     NOT a precise InstanceStart-to-ready measurement. v0.2 instrumentation
#     would split out phase_12b's deadline-bounded timing precisely.
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

mkdir -p "$(dirname "$RESULT_CSV")"
if [[ ! -f "$RESULT_CSV" ]]; then
    echo "timestamp,kind,load,attempt,launch_ms,exit" > "$RESULT_CSV"
fi

# Build the CLI once.
cargo build --release -p m80-cli >/dev/null

run_cell() {
    local kind="$1"
    local load="$2"
    local image_dir="$3"
    local stress_pid=""

    if [[ "$load" == "loaded" ]]; then
        echo "  starting stress-ng --cpu $STRESS_PROCS"
        stress-ng --cpu "$STRESS_PROCS" --quiet >/dev/null 2>&1 &
        stress_pid=$!
        sleep 1
    fi

    local results=()
    local total=$((N + WARMUP))
    for i in $(seq 1 "$total"); do
        sudo IMAGE_BUILD_DIR="$image_dir" \
             ./target/release/m80 cleanup >/dev/null 2>&1 || true

        local start_ns end_ns elapsed_ms exit_code
        start_ns=$(date +%s%N)
        if timeout 90 sudo IMAGE_BUILD_DIR="$image_dir" \
                M80_KERNEL_IMAGE="$image_dir/vmlinux" \
                M80_ROOTFS_IMAGE="$image_dir/output.ext4" \
                ./target/release/m80 launch \
                --network noegress -- /bin/echo bench-$i >/dev/null 2>&1; then
            exit_code=0
        else
            exit_code=$?
        fi
        end_ns=$(date +%s%N)
        elapsed_ms=$(( (end_ns - start_ns) / 1000000 ))

        if (( i > WARMUP )); then
            results+=("$elapsed_ms")
            echo "$(date -Iseconds),$kind,$load,$i,$elapsed_ms,$exit_code" >> "$RESULT_CSV"
        fi
    done

    if [[ -n "$stress_pid" ]]; then
        kill "$stress_pid" 2>/dev/null || true
        wait "$stress_pid" 2>/dev/null || true
    fi

    summarize "$kind" "$load" "${results[@]}"
}

summarize() {
    local kind="$1" load="$2"
    shift 2
    local sorted
    sorted=$(printf '%s\n' "$@" | sort -n)
    local count
    count=$(printf '%s\n' "$sorted" | wc -l)
    local p50_idx p95_idx
    p50_idx=$(( count / 2 ))
    p95_idx=$(( count * 95 / 100 ))
    if (( p50_idx == 0 )); then p50_idx=1; fi
    if (( p95_idx == 0 )); then p95_idx=1; fi
    local p50 p95 max
    p50=$(printf '%s\n' "$sorted" | sed -n "${p50_idx}p")
    p95=$(printf '%s\n' "$sorted" | sed -n "${p95_idx}p")
    max=$(printf '%s\n' "$sorted" | tail -1)
    printf '  %-8s %-6s  P50=%5sms  P95=%5sms  MAX=%5sms  (n=%s)\n' \
        "$kind" "$load" "$p50" "$p95" "$max" "$count"
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

echo
echo "Results appended to $RESULT_CSV"
