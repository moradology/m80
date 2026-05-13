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
    grep -Eq "(^|[^[:alnum:]_])${token}([^[:alnum:]_]|$)" <<<"$text"
}

# --- Help text mentions the new env vars ---
echo "=== help text ==="
help_text="$(bash scripts/bench-cold-launch.sh --help 2>&1 || true)"
for v in N WARMUP KIND SKIP_LOADED KERNEL_KIND EGRESS SWEEP CONCURRENT TASKSET CPU_GOVERNOR DRY_RUN; do
    if contains_token "$v" "$help_text"; then
        note ok "help mentions $v"
    else
        note FAIL "help missing $v"
    fi
done
if grep -q -- "--cold-isolation" <<<"$help_text"; then
    note ok "help mentions --cold-isolation"
else
    note FAIL "help missing --cold-isolation"
fi
if grep -q -- "--dry-run" <<<"$help_text"; then
    note ok "help mentions --dry-run"
else
    note FAIL "help missing --dry-run"
fi

# --- --dry-run prints planned cells and skips execution ---
echo "=== --dry-run ==="
dry_out="$(bash scripts/bench-cold-launch.sh --dry-run 2>&1 || true)"
if grep -q "dry-run" <<<"$dry_out"; then
    note ok "--dry-run banner present"
else
    note FAIL "--dry-run banner missing"
fi
# --dry-run should not invoke sudo or cargo (look for actual execution markers,
# not mentions in the plan banner).
if grep -qE "(^\+ sudo|^\+ cargo|sudo:|password|Compiling )" <<<"$dry_out"; then
    note FAIL "--dry-run still invoking sudo/cargo"
else
    note ok "--dry-run does not invoke sudo/cargo"
fi

# --- SWEEP=vcpu emits a sweep CSV ---
echo "=== SWEEP=vcpu --dry-run ==="
sweep_out="$(SWEEP=vcpu SWEEP_VALUES=1,2,4 bash scripts/bench-cold-launch.sh --dry-run 2>&1 || true)"
if grep -qE "sweep.*vcpu.*1,2,4|vcpu.*=.*1" <<<"$sweep_out"; then
    note ok "SWEEP=vcpu planned in dry-run"
else
    note FAIL "SWEEP planning missing from dry-run"
fi

# --- CONCURRENT=4 --dry-run ---
echo "=== CONCURRENT=4 --dry-run ==="
conc_out="$(CONCURRENT=4 bash scripts/bench-cold-launch.sh --dry-run 2>&1 || true)"
if grep -qE "concurrent.*4|CONCURRENT=4" <<<"$conc_out"; then
    note ok "CONCURRENT=4 planned in dry-run"
else
    note FAIL "CONCURRENT plan missing"
fi

# --- WARMUP override visible ---
echo "=== WARMUP=5 --dry-run ==="
warm_out="$(WARMUP=5 bash scripts/bench-cold-launch.sh --dry-run 2>&1 || true)"
if grep -qE "warmup=5|WARMUP=5" <<<"$warm_out"; then
    note ok "WARMUP=5 visible in dry-run"
else
    note FAIL "WARMUP env var not surfaced"
fi

# --- --cold-isolation surface in dry-run ---
echo "=== --cold-isolation --dry-run ==="
cold_out="$(bash scripts/bench-cold-launch.sh --cold-isolation --dry-run 2>&1 || true)"
if grep -qE "drop_caches|cold-isolation" <<<"$cold_out"; then
    note ok "--cold-isolation reflected in dry-run"
else
    note FAIL "--cold-isolation not surfaced in dry-run"
fi

# --- TASKSET env in dry-run ---
echo "=== TASKSET=0,1 --dry-run ==="
tset_out="$(TASKSET=0,1 bash scripts/bench-cold-launch.sh --dry-run 2>&1 || true)"
if grep -qE "taskset|TASKSET=0,1" <<<"$tset_out"; then
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

# --- bench-extras.sh modes ---
echo "=== bench-extras.sh modes ==="
extras_help="$(bash scripts/bench-extras.sh --help 2>&1)"
for mode in --throughput --memory --teardown --boot-decomp --long-tail --density; do
    if grep -q -- "$mode" <<<"$extras_help"; then
        note ok "bench-extras --help advertises $mode"
    else
        note FAIL "bench-extras --help missing $mode"
    fi
done

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
