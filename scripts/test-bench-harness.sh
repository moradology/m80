#!/usr/bin/env bash
# Plumbing tests for bench-cold-launch.sh / bench-extras.sh.
#
# Real-KVM bench runs require sudo + image build + minutes per attempt;
# these tests only cover the orchestration plumbing: --help text,
# --dry-run plan output, env-var surfacing, and the bench-extras --help
# mode catalog. Real-data correctness is verified by actually running
# `./scripts/bench-cold-launch.sh` against a privileged host.
#
# Requirements:
#   - shellcheck on PATH for the local script-lint gate.

set -euo pipefail

cd "$(dirname "$0")/.."

fail=0
pass=0
tmp_root="$(mktemp -d)"
trap 'rm -rf "$tmp_root"' EXIT

note() {
    local kind="$1" msg="$2"
    if [[ "$kind" == "ok" ]]; then
        pass=$((pass + 1))
        printf '  ok   %s\n' "$msg"
    else
        fail=$((fail + 1))
        printf '  FAIL %s\n' "$msg" >&2
    fi
}

contains_token() {
    local token="$1" text="$2"
    rg -q "(^|[^[:alnum:]_])${token}([^[:alnum:]_]|$)" <<<"$text"
}

contains_fixed() {
    local needle="$1" text="$2"
    rg -q --fixed-strings -- "$needle" <<<"$text"
}

contains_regex() {
    local pattern="$1" text="$2"
    rg -q -- "$pattern" <<<"$text"
}

# --- Help text mentions the new env vars ---
echo "=== help text ==="
help_text="$(bash scripts/bench-cold-launch.sh --help 2>&1 || true)"
for v in N WARMUP KIND SKIP_LOADED KERNEL_KIND EGRESS SWEEP CONCURRENT TASKSET CPU_GOVERNOR DRY_RUN PERF_STAT PERF_STAT_SECONDS BENCH_ARTIFACT_DIR; do
    if contains_token "$v" "$help_text"; then
        note ok "help mentions $v"
    else
        note FAIL "help missing $v"
    fi
done
if contains_fixed "--cold-isolation" "$help_text"; then
    note ok "help mentions --cold-isolation"
else
    note FAIL "help missing --cold-isolation"
fi
if contains_fixed "--dry-run" "$help_text"; then
    note ok "help mentions --dry-run"
else
    note FAIL "help missing --dry-run"
fi

# --- --dry-run prints planned cells and skips execution ---
echo "=== --dry-run ==="
dry_out="$(bash scripts/bench-cold-launch.sh --dry-run 2>&1 || true)"
if contains_fixed "dry-run" "$dry_out"; then
    note ok "--dry-run banner present"
else
    note FAIL "--dry-run banner missing"
fi
# --dry-run should not invoke sudo or cargo (look for actual execution markers,
# not mentions in the plan banner).
if contains_regex "(^\+ sudo|^\+ cargo|sudo:|password|Compiling )" "$dry_out"; then
    note FAIL "--dry-run still invoking sudo/cargo"
else
    note ok "--dry-run does not invoke sudo/cargo"
fi

# --- SWEEP=vcpu emits a sweep CSV ---
echo "=== SWEEP=vcpu --dry-run ==="
sweep_out="$(SWEEP=vcpu SWEEP_VALUES=1,2,4 bash scripts/bench-cold-launch.sh --dry-run 2>&1 || true)"
if contains_regex "sweep.*vcpu.*1,2,4|vcpu.*=.*1" "$sweep_out"; then
    note ok "SWEEP=vcpu planned in dry-run"
else
    note FAIL "SWEEP planning missing from dry-run"
fi

# --- CONCURRENT=4 --dry-run ---
echo "=== CONCURRENT=4 --dry-run ==="
conc_out="$(CONCURRENT=4 bash scripts/bench-cold-launch.sh --dry-run 2>&1 || true)"
if contains_regex "concurrent.*4|CONCURRENT=4" "$conc_out"; then
    note ok "CONCURRENT=4 planned in dry-run"
else
    note FAIL "CONCURRENT plan missing"
fi

# --- WARMUP override visible ---
echo "=== WARMUP=5 --dry-run ==="
warm_out="$(WARMUP=5 bash scripts/bench-cold-launch.sh --dry-run 2>&1 || true)"
if contains_regex "warmup=5|WARMUP=5" "$warm_out"; then
    note ok "WARMUP=5 visible in dry-run"
else
    note FAIL "WARMUP env var not surfaced"
fi

# --- --cold-isolation surface in dry-run ---
echo "=== --cold-isolation --dry-run ==="
cold_out="$(bash scripts/bench-cold-launch.sh --cold-isolation --dry-run 2>&1 || true)"
if contains_regex "drop_caches|cold-isolation" "$cold_out"; then
    note ok "--cold-isolation reflected in dry-run"
else
    note FAIL "--cold-isolation not surfaced in dry-run"
fi

# --- TASKSET env in dry-run ---
echo "=== TASKSET=0,1 --dry-run ==="
tset_out="$(TASKSET=0,1 bash scripts/bench-cold-launch.sh --dry-run 2>&1 || true)"
if contains_regex "taskset|TASKSET=0,1" "$tset_out"; then
    note ok "TASKSET visible in dry-run"
else
    note FAIL "TASKSET not surfaced"
fi

# --- EGRESS=outbound --dry-run ---
echo "=== EGRESS=outbound --dry-run ==="
egress_out="$(EGRESS=outbound bash scripts/bench-cold-launch.sh --dry-run 2>&1 || true)"
if [[ "$egress_out" == *"EGRESS=outbound"* ]]; then
    note ok "EGRESS=outbound planned in dry-run"
else
    note FAIL "EGRESS plan missing"
fi

# --- PERF_STAT env in dry-run ---
echo "=== PERF_STAT=1 --dry-run ==="
perf_plan="$(PERF_STAT=1 PERF_STAT_SECONDS=3 bash scripts/bench-cold-launch.sh --dry-run 2>&1 || true)"
if [[ "$perf_plan" == *"PERF_STAT=1"* && "$perf_plan" == *"PERF_STAT_SECONDS=3"* ]]; then
    note ok "PERF_STAT plan visible in dry-run"
else
    note FAIL "PERF_STAT plan missing"
fi

perf_concurrent_out="$(PERF_STAT=1 CONCURRENT=2 bash scripts/bench-cold-launch.sh --dry-run 2>&1 || true)"
if [[ "$perf_concurrent_out" == *"PERF_STAT=1 does not support CONCURRENT>0"* ]]; then
    note ok "PERF_STAT rejects concurrent launches"
else
    note FAIL "PERF_STAT concurrent guard missing"
fi

# --- bench-extras.sh modes ---
echo "=== bench-extras.sh modes ==="
extras_help="$(bash scripts/bench-extras.sh --help 2>&1)"
for mode in --throughput --memory --teardown --boot-decomp --long-tail --density; do
    if contains_fixed "$mode" "$extras_help"; then
        note ok "bench-extras --help advertises $mode"
    else
        note FAIL "bench-extras --help missing $mode"
    fi
done

# --- warmup rows are excluded from phase snapshots ---
echo "=== warmup phase discard ==="
fake_bin="$tmp_root/fake-bin"
fake_image="$tmp_root/minimal-image"
fake_artifacts="$tmp_root/artifacts"
mkdir -p "$fake_bin" "$fake_image" "$fake_artifacts"
: > "$fake_image/vmlinux"
printf '{}\n' > "$fake_image/output.ext4.manifest.json"

cat > "$fake_bin/sudo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
while [[ $# -gt 0 ]]; do
    case "$1" in
        *=*) shift ;;
        *) break ;;
    esac
done
exec "$@"
EOF
chmod +x "$fake_bin/sudo"

cat > "$fake_bin/perf" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
out=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        -o)
            out="$2"
            shift 2
            ;;
        *)
            shift
            ;;
    esac
done
if [[ -z "$out" ]]; then
    exit 2
fi
cat > "$out" <<'RAW'
# started on fixture
0.100000000,10,,dTLB-load-misses,100,100.00,,
0.100000000,20,,iTLB-load-misses,100,100.00,,
0.100000000,30,,cache-misses:u,100,100.00,,
RAW
EOF
chmod +x "$fake_bin/perf"

cat > "$fake_bin/m80" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "${1:-}" in
    cleanup)
        if [[ -n "${M80_RUN_ROOT:-}" && "$M80_RUN_ROOT" == /tmp/* ]]; then
            rm -rf "$M80_RUN_ROOT"/*
        fi
        exit 0
        ;;
    run)
        last="${*: -1}"
        attempt="${last#bench-}"
        vm_id="mock-$attempt"
        if [[ -n "${M80_RUN_ROOT:-}" ]]; then
            mkdir -p "$M80_RUN_ROOT/$vm_id"
            printf '{"schema_version":1,"jailer_pid":%s,"firecracker_pid":%s}\n' "$$" "$$" \
                > "$M80_RUN_ROOT/$vm_id/jailer-state.json"
        fi
        sleep 0.05
        echo "M80_PHASE name=phase_12b_ready_accept elapsed_us=$((1000 + attempt))" >&2
        exit 0
        ;;
    *)
        exit 2
        ;;
esac
EOF
chmod +x "$fake_bin/m80"

mock_out="$(
    PATH="$fake_bin:$PATH" \
    BENCH_ARTIFACT_DIR="$fake_artifacts" \
    IMAGE_BUILD_DIR_MINIMAL="$fake_image" \
    M80_RUN_ROOT="$tmp_root/run-root" \
    M80_BIN="$fake_bin/m80" \
    N=2 WARMUP=1 KIND=minimal SKIP_LOADED=1 \
    bash scripts/bench-cold-launch.sh 2>&1
)" || {
    printf '%s\n' "$mock_out" >&2
    note FAIL "mock bench run failed"
}

phase_rows="$fake_artifacts/cold-launch-phases.csv"
phase_count="$(
    python3 - "$phase_rows" <<'PY'
import csv
import sys
with open(sys.argv[1]) as f:
    print(sum(1 for _ in csv.DictReader(f)))
PY
)"
snapshot_count="$(
    python3 - "$fake_artifacts/snapshots/latest.json" <<'PY'
import json
import sys
with open(sys.argv[1]) as f:
    data = json.load(f)["data"]
print(data["phases"]["minimal"]["idle"]["phase_12b_ready_accept"]["count"])
PY
)"
if [[ "$phase_count" == "2" && "$snapshot_count" == "2" ]]; then
    note ok "warmup phase rows excluded from CSV and snapshot"
else
    note FAIL "warmup phase rows leaked into CSV/snapshot"
fi
if rg -q ',1,phase_12b_ready_accept,' "$phase_rows"; then
    note FAIL "warmup attempt 1 was recorded as a phase sample"
else
    note ok "warmup attempt 1 absent from phase samples"
fi

# --- PERF_STAT captures only post-warmup attempts ---
echo "=== PERF_STAT mock run ==="
perf_artifacts="$tmp_root/perf-artifacts"
mkdir -p "$perf_artifacts"
perf_out="$(
    PATH="$fake_bin:$PATH" \
    BENCH_ARTIFACT_DIR="$perf_artifacts" \
    IMAGE_BUILD_DIR_MINIMAL="$fake_image" \
    M80_RUN_ROOT="$tmp_root/perf-run-root" \
    M80_BIN="$fake_bin/m80" \
    PERF_STAT=1 PERF_STAT_SECONDS=1 \
    N=2 WARMUP=1 KIND=minimal SKIP_LOADED=1 \
    bash scripts/bench-cold-launch.sh 2>&1
)" || {
    printf '%s\n' "$perf_out" >&2
    note FAIL "PERF_STAT mock bench run failed"
}
perf_counts="$(
    python3 - "$perf_artifacts/perf-counters.csv" <<'PY'
import csv
import sys
from collections import Counter
with open(sys.argv[1]) as f:
    rows = list(csv.DictReader(f))
print(len(rows), sorted(Counter(row["attempt"] for row in rows)), sorted(row["event"] for row in rows[:3]))
PY
)"
if [[ "$perf_counts" == "6 ['2', '3'] ['cache-misses', 'dTLB-load-misses', 'iTLB-load-misses']" ]]; then
    note ok "PERF_STAT rows captured for measured attempts only"
else
    note FAIL "PERF_STAT rows wrong: $perf_counts"
fi

# --- bench-density-extended.sh dry-run ---
echo "=== bench-density-extended.sh --dry-run ==="
density_out="$(bash scripts/bench-density-extended.sh --dry-run 2>&1 || true)"
if [[ "$density_out" == *"bench-density-extended"* &&
      "$density_out" == *"no-egress ladder"* &&
      "$density_out" == *"outbound ladder"* ]]; then
    note ok "bench-density-extended dry-run prints the sweep plan"
else
    note FAIL "bench-density-extended dry-run plan missing"
fi

# --- bench-restore-cold.sh dry-run ---
echo "=== bench-restore-cold.sh --dry-run ==="
restore_out="$(bash scripts/bench-restore-cold.sh --dry-run 2>&1 || true)"
if [[ "$restore_out" == *"bench-restore-cold"* &&
      "$restore_out" == *"FILE_READ_SAMPLES"* &&
      "$restore_out" == *"cold-restore-N"* ]]; then
    note ok "bench-restore-cold dry-run prints the restore plan"
else
    note FAIL "bench-restore-cold dry-run plan missing"
fi

# --- shellcheck gate ---
echo "=== shellcheck ==="
if shellcheck -s bash scripts/*.sh; then
    note ok "shellcheck passes for scripts/*.sh"
else
    note FAIL "shellcheck failed for scripts/*.sh"
fi

echo
echo "=== summary: $pass pass / $fail fail ==="
[[ $fail -eq 0 ]]
